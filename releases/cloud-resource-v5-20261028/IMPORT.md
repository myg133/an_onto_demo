# cloud-resource 本体交付包（取自 UAT · 2026-10-28）

> 一句话：**这是 UAT 上正在服务的本体（`cloud-resource` 第 5 版）的原始构建包**
> —— `.ontokbundle`，184,592 字节，sha256 `8ec983f3…`。线上按下面四步导入即可。

---

## 一 · 包里是什么

| 项 | 值 |
| --- | --- |
| 文件 | `cloud-resource-v5.ontokbundle` |
| 大小 / sha256 | 184,592 B · `8ec983f3fad2145c6561f794b8cfc366934424c39feb4fbd98aba58f178f92bd` |
| 包格式 | `bundle_version = "1.0"`（store-only zip，2 个 entry） |
| 项目名 | **`cloud-resource`**（⚠️ 线上项目必须叫这个，见 §四） |
| 打包时的版本 | `ontology_seq = 5` · `built_at = 2026-10-27T03:16:34Z` |
| 内容 | 30 个类 · 34 条关系 · 752 个属性 |
| 需要 DSN | **否**（`requires_dsn` 未置位，纯本体，不连源库） |

包内只有两个文件：`manifest.json` + `ontology/snapshot.json`。
**不含**：访问映射（AM）、声明（policy / tenancy / exclusions / recipe）、源库 schema、DSN、token、任何 key。

---

## 二 · 怎么证明它 = UAT 正在服务的本体

| 证据 | 结果 |
| --- | --- |
| 字节来源 | UAT 元数据库 `project_bundle_payloads.data`（UAT **自己收到并解析过**的那份字节），整包 sha256 与 `project_bundles.bundle_sha256` 一致 |
| 自述版本 | 包内快照 `version_id = 271723f8-…`、`seq = 5` —— 正是 UAT 的 `active_version` |
| 内容比对 | 该包在 UAT 导入出的 v6 与生效中的 v5，在 `object_types` / `link_types` 上**除自增 id 外逐列相同**（30 / 34 行） |
| 计数比对 | 30 / 34 / 752 与 UAT `/api/catalog` 的 `counts` 一致 |
| 格式自检 | `ontok-builder bundle verify`（0.3.1）→ `ok: true`，0 problems / 0 warnings |

> 也就是说：**包 = UAT 第 5 版本体**，不是从别处重新生成的近似物。

---

## 三 · 上线前先确认这 5 件事（缺一条会「看起来成功、实际不可用」）

| # | 检查 | 不满足会怎样 |
| --- | --- | --- |
| 1 | `ONTOK_SECRET_KEY`（64 hex）已注入 | `query serve` **拒绝启动**（CrashLoopBackOff） |
| 2 | token 表里有一条 `kind: "admin"`（不绑 project） | 能起，但**写面全 403 `key.not_admin`** —— 包根本传不上去 |
| 3 | 线上**没有**同名项目，或同名项目是空的 | 建项目 409 `project.exists`；传别人的包 409 `project.mismatch` |
| 4 | 单项目模式（`--project/--version`）先想好版本号 | 导入后的版本号是**线上库里的下一个号**（空库就是 **v1**，不是 v5）；`--version` 钉错 ⇒ 503 |
| 5 | 多项目模式（两个参数都不给）时，每条 `kind:"query"` 的 token 都要带 `project` | 整份 token 表被拒，服务起不来 |

---

## 四 · 导入四步（写面全部用 **admin** key）

```bash
B=https://<线上域名>            # 或先 kubectl port-forward svc/ontok-query 18800:8800
A=<admin token>
PKG=cloud-resource-v5.ontokbundle

# ① 建项目 —— 名字必须与包里的 project 一致（cloud-resource）
curl -sS -X POST $B/api/projects \
  -H "Authorization: Bearer $A" -H 'Content-Type: application/json' \
  -d '{"project":"cloud-resource","label":"腾讯云资源本体","key_label":"数字员工：运维助手"}'
#   201 ⇒ 记下响应里的 "key"：这是给数字员工的 query key（明文只出现这一次，
#          之后可用 GET /api/projects/cloud-resource/key 再看）

# ② 上传包（⚠️ 原始字节，不是 multipart 表单）
curl -sS -X POST $B/api/projects/cloud-resource/bundles \
  -H "Authorization: Bearer $A" -H 'Content-Type: application/octet-stream' \
  --data-binary @$PKG
#   ⇒ {"bundle_id":"bdl_…","state":"uploaded","bytes":184592,…}

# ③ 解析入库（不冻结）
curl -sS -X POST $B/api/projects/cloud-resource/bundles/<bundle_id>/import \
  -H "Authorization: Bearer $A"
#   ⇒ {"version":1,"frozen":false,"counts":{"entity_types":30,"link_types":34,"properties":752,…}}
#   ⚠️ 版本号以这里返回的为准，别假设是 5

# ④ 激活（冻结 + 生效）—— 漏了这步，数字员工拿到的是 503
curl -sS -X POST $B/api/projects/cloud-resource/bundles/<bundle_id>/activate \
  -H "Authorization: Bearer $A"
#   ⇒ {"frozen":true,"active_version_id":"…"}
```

用 Studio 页面也可以：**建项目 → 上传包 → 解析 → 激活**（Studio 走的就是这四条端点）。

---

## 五 · 验收（四条，缺一条都别宣布成功）

```bash
Q=<① 返回的 query key>

# ① 服务活着，且版本 = ③ 返回的那个号
curl -sS $B/api/health
#   期望：project=cloud-resource、ontology_version=1（或实际上线的号）

# ② 计数对得上 UAT：30 类 / 34 关系 / 752 属性
curl -sS -H "Authorization: Bearer $Q" "$B/api/catalog?page_size=1"
#   期望：{"counts":{"classes":30,"properties":752,"relations":34,…}}

# ③ 写面隔离：admin 200、query key 403
curl -sS -o /dev/null -w '%{http_code}\n' -H "Authorization: Bearer $A" $B/api/projects   # 200
curl -sS -o /dev/null -w '%{http_code}\n' -H "Authorization: Bearer $Q" $B/api/projects   # 403

# ④ 本体 TTL 与 UAT 逐字节一致（本目录 ontology/uat-served-customer-view.ttl 是对照件）
curl -sS -H "Authorization: Bearer $Q" $B/api/ontology.ttl -o prod.ttl
#   sha256 应为 01dd090f76e64439300206ad571a32845648aa44e6d50504549613ca1b32fdc0
```

> ④ 的 sha 是 UAT 当前对外服务的客户视图 TTL（210,610 B）。
> 线上如果注入了**不同的** AM/声明，TTL 结构不变、`access` 块可能不同 ⇒ 以计数 + 类清单为准。

---

## 六 · 撞墙对照表

| 现象 | 原因 | 处置 |
| --- | --- | --- |
| `403 key.not_admin` | 用了 query key 调写面 | 换 `kind:"admin"` 的 token |
| `409 project.exists` | 线上已有 `cloud-resource` | 改用已有项目继续 ②；或删空后重建（v1 无删除端点，需在库里处理） |
| `409 bundle.project.mismatch` | 线上项目名 ≠ 包里的 `cloud-resource` | 要么项目改名成 `cloud-resource`，要么让我按新名重打一个包（改动只在 manifest，1 分钟） |
| `415 bundle.upload.invalid` | 用了 `-F file=@…`（multipart） | 改 `--data-binary @…` + `application/octet-stream` |
| `503 project.no_active_version` | 只做了 ③ 没做 ④ | 调 `activate` |
| 单项目模式 `503` / 版本不存在 | 部署参数 `--version` 钉的不是 ③ 的号 | 改 Deployment 的 `ONTOK_VERSION` 后滚动重启 |
| `/api/sparql` 恒 503 | 本体模式（`--ontology-only`）本来就不查库 | 要查库 ⇒ 另配 AM + `--schema` + DSN，且去掉 `--ontology-only` |

---

## 七 · 包的边界（这次只交付「本体」）

| 项 | 在包里吗 | 说明 |
| --- | --- | --- |
| 类 / 关系 / 属性（本体） | ✅ | 全部在 `ontology/snapshot.json` |
| 访问映射 AM（14 个类的取数契约） | ❌ | UAT 是**单独 ConfigMap** 挂的（`Deploy/k8s/overlays/uat-ops360/am-tencent.json`）。线上要 `catalog` 里出现 `access.kind/api`，得自己挂一份 |
| 声明（policy / tenancy / exclusions） | ❌ | ConfigMap `ontok-config`，线上按需 |
| 源库 schema / DSN | ❌ | 纯本体不需要 |
| token / key | ❌ | 建项目时新发，**不要**从 UAT 复制 |
| `ONTOK_SECRET_KEY` | ❌ | 线上自己生成、自己保管（与库一起备份） |

---

## 八 · 回滚

- 导入是**追加**：新版本不覆盖旧版本，`activate` 只挪指针。
- 回滚 = 把 `active_version_id` 指回上一版：对**旧包**再调一次 `activate`（或 `ontok version freeze/activate`）。
- 包文件本身留在 `Deploy/releases/cloud-resource-v5-20261028/`，二进制可归档到制品库（sha256 见上）。
