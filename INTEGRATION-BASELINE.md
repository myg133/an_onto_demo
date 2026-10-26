# 接入基线（2026-10-26）· **接入期间冻结**

> 给接入方（Axon / 数字员工）与联调同学：**这两周内以下内容不变**。
> 变动一律等接入验收通过后再发新版本。

---

## 一 · 冻结清单

| 项 | 值 |
| --- | --- |
| **代码 tag** | `v0.1.8`（develop `6682621`） |
| **部署 tag** | `deploy-uat-2026-10-26`（deploy `6edde54`） |
| **镜像** | `aigccontainer.azurecr.cn/an_onto/ontok:0.1.8`<br>digest `sha256:ee441aca0a2b5b46a60500018ff59564bfd143b8b1308bbc458239dcaaee7cdf` |
| **本体** | `cloud-resource` **v3**（已冻结）· 30 类 / 34 关系 / 752 属性 |
| **TTL** | 209,429 B · sha256 `79741a7fecc383562dc9f0ccfd69a149…`（与 `GET /api/ontology.ttl` 的 ETag 前 16 位一致 `79741a7fecc38356`） |
| **access mapping** | `am-tencent.json` · 14 entity / 441 属性 · **已签** `michael@2026-10-26` · `mapping_version: 1` |
| **契约版本** | `contract_version: 1`（`@pending.get` = `{class, filter, pushdown, ref}`） |
| **运行模式** | `--ontology-only`（**不接业务库**） |
| **公网入口** | `https://an-notol.mic-scfw.cloudns.org`（Cloudflare Tunnel sidecar） |
| **工具** | `catalog` / `describe` / `sparql_query` / `sample`；资源 `ontok://ontology/{customer,model}.ttl` |

---

## 二 · 能力矩阵（接入方最需要知道的三件事）

| 问题 | 答案 |
| --- | --- |
| 能查到数据吗？ | **不能**。这套不接库。 |
| 那 14 个 api 类怎么拿数？ | 平台给 `@pending` + `get`，**你按 `get` 自己去调腾讯云** |
| 那另外 16 个类呢？ | 有概念、**暂无取数契约** ⇒ 查了会 `503`。这是"还没接上"，不是"没有这个东西" |

**14 个有契约的类**：`cvm_instance` `vpc` `subnet` `security_group` `security_group_rule` `cbs_disk` `cbs_snapshot` `clb_load_balancer` `clb_listener` `cdb_instance` `db_backup` `ssl_certificate` `alarm_policy` `cost_detail_bill`

---

## 三 · 接入期间**已知的四个限制**（不是缺陷，已排期）

| # | 限制 | 影响 | 应对 |
| --- | --- | --- | --- |
| 1 | **`get` 里没有完整取数配方**（缺 action / version / host / iterator / 字段映射） | 接入方要自己按腾讯云 API 写请求 | 用 `ref` 找我们要**已签映射**；或先照 `skills/ontok-query/SKILL.md` 附录 A 实现 |
| 2 | **本体模式下 `get.filter` 常为 `{}`** | 无法从查询里推出取数标识 | ⛔ 不要全量拉；先按标识属性取，或至少用 `pushdown` 收窄 |
| 3 | **16 个类无取数契约** | 查它们返回 503 | 按需提需求，我们补接口语料 + 映射 |
| 4 | **mapping 有两份形状**（`freshness` 对象 vs 字符串） | 部署时用临时投影文件 | 对使用方**透明**，不用管 |

**这四条都在 REQ-152 里统一解决**（下一版）。接入期间请按"应对"那一列做。

---

## 四 · 接入方怎么自查（三条命令）

```bash
# ① 通不通
curl -sS "$ONTOK_BASE_URL/api/health"

# ② 有没有 api 类
curl -sS -H "Authorization: Bearer $ONTOK_TOKEN" "$ONTOK_BASE_URL/api/describe?name=cvm_instance" | grep -o '"access":{[^}]*}'

# ③ @pending 对不对
curl -sS -X POST "$ONTOK_BASE_URL/api/sparql" \
  -H "Authorization: Bearer $ONTOK_TOKEN" -H "Content-Type: application/json" \
  -H 'Accept: application/ld+json' \
  -d '{"query":"SELECT ?c WHERE { ?c a :cvm_instance } LIMIT 1"}'
```

**③ 的期望**：`@graph: []` + `@pending: [1]` + `meta.complete: false`。

---

## 五 · 冻结期间的规则

| 允许 | 不允许 |
| --- | --- |
| 接入方联调、提问题、要 16 类的补充映射 | 改本体（v3 已冻结，改就要 v4） |
| 我们改**文档/skill**（不影响二进制） | 合并新需求到 `develop` 后发版 |
| 我们做 REQ-152 的**设计与开发**（在 feature 分支上） | 把 REQ-152 合进 `v0.1.8` |

**发新版本的门槛**：接入方验收通过 → 我们合 REQ-152 → 发 `v0.2.0` → 灰度切。

---

## 六 · 一句话

> **接入期间你手里这套是"钉死的"**：镜像、本体、映射、契约都不动。
> 有问题**改用法**，不改基线；确实要改，走新版本。
