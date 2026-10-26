# 接入基线（**v0.2.1** · 2026-10-26）

> 给接入方（Axon / 数字员工）与联调同学：**这套是当前冻结的接入基线**。
> 上一版 `v0.1.8`（`get` 只有 4 个键）已被 `v0.2.0` 取代。

---

## 一 · 冻结清单

| 项 | 值 |
| --- | --- |
| **代码 tag** | `v0.2.1`（develop `aed51c6`，含 REQ-152 阶段1+2+CP4 + REQ-154 四小修） |
| **部署 tag** | `deploy-uat-v0.2.1` |
| **镜像** | `aigccontainer.azurecr.cn/an_onto/ontok:0.2.1`<br>digest `sha256:02bbdea3856fdbcc7aa0e0ae1702192123cc6cd436de808678c6d879ad1f7e40` |
| **本体** | `cloud-resource` **v3**（已冻结）· 30 类 / 34 关系 / 752 属性 |
| **TTL** | 209,429 B · sha256 `79741a7f…`（**契约升级不影响本体输出**） |
| **access mapping** | `am-tencent.json` · **`contract_version: 2`** · 14 entity / 441 属性<br>sha256 `80fce221d5f803487983cb3afe2f9d2d…` · 已签 `michael@2026-10-26` |
| **契约** | **v2**：`@pending.get` = **10 个键** · `meta.contract_version` = **2** |
| **运行模式** | `--ontology-only`（**不接业务库**） |
| **公网入口** | `https://an-notol.mic-scfw.cloudns.org` |
| **内网入口** | `https://an-onto.local.anspire.cn`（ingress 已从 `ontok-ops360` 更名为 `ontok`） |

---

## 二 · v0.2.1 相对 v0.2.0 的变化（**只有两处，都是加法**）

| 项 | v0.2.0 | v0.2.1 |
| --- | --- | --- |
| `meta.contract_version` | **没有** | **有**（v2 映射 ⇒ `2`；v1 ⇒ `1`）——用它判断"映射升没升"，⛔ 别再数 `get` 的键 |
| `400 NOT_DB_SOURCE` 的 hint | 只列 4 个键 | 列全（v2 映射点 10 个）|

**实测**：`04-pending` 3366 B（含 `contract_version: 2`）· `05-rows400` 1074 B。
`get` 仍是 **10 个键**，一个不多一个不少。

## 三 · v0.2.0 相对 v0.1.8 的**唯一行为变化**

```diff
  GET /api/sparql  →  @pending[0].get
-   4 个键：class, filter, pushdown, ref
+  10 个键：class, filter, pushdown, ref,
+           source, operation, request, iterator, fields, auth
```

**其余一切不变**：`catalog` / `describe` / `health` / TTL / 503 / 400 逐字节相同（8 case 已对拍）。

### 10 个键怎么用（**照 `RECIPE.md` 做，别猜**）

| 键 | 给你什么 |
| --- | --- |
| `source` | `base_url` + `auth_mode` |
| `operation` | `method` / `path` / `headers{}`（含 `{action}`/`{version}`/`{timestamp}`/`{region}`）/ `vars{}` |
| `request` | `template{}` / `required[]` / `paging{}` / **`defaults{}`**（`source=default` 的值在这） |
| `iterator` | 列表在响应里的路径 |
| `fields` | 本体属性 → 响应字段（拍平成字符串） |
| `auth` | `credential_env`（**只有变量名**）/ `signed_headers` / `timestamp` / `scope` |

**签名怎么实现**：见 `skills/ontok-query/AUTH-SPEC.md`
**配方怎么用（含剪枝四规则、分页类型、required 语义）**：见 `skills/ontok-query/RECIPE.md`

---

## 四 · 实测过的部分（接入方可直接依赖）

| 类 | 结果 |
| --- | --- |
| `cvm_instance` | ✅ 200 · 3 台（zone=ap-guangzhou-5）· 字段 **41/41** |
| `vpc` | ✅ 200 · 10 个 · 字段 **17/17** |
| `ssl_certificate` | ✅ 200 · 63 张 · 字段 **63/63** |
| `alarm_policy` | ✅ 200 · 236 条 · 字段 11/15（`Module` 来自 `request.defaults`） |

**验证方式**：不看我们的执行器，**只按 `get` 自己实现客户端**去真调（用的是只读验证账号）。

**反证也做了**：`cvm_instance` 用 `ap-guangzhou-5` → 3 台；用 `ap-guangzhou-2` → **0 台** ⇒ 过滤/分页真的生效。

---

## 五 · **没验过的部分**（接入方注意）

| 类 | 状态 |
| --- | --- |
| `subnet` / `security_group` / `cbs_disk` / `cbs_snapshot` / `clb_load_balancer` / `cdb_instance` | 结构对、能返回，但**最多只验到"能出数据"** |
| `db_backup` / `clb_listener` / `security_group_rule` / `cost_detail_bill` | **需要业务 ID（`InstanceId`/`LoadBalancerId`/`SecurityGroupId`/`Month`）**，没有真调过；缺 `required` 时**客户端应拒发** |

**字段覆盖 <100% 的类**：`subnet` 17/18、`cbs_snapshot` 23/24、`alarm_policy` 11/15
（原因：`X.N` 数组属性不支持下标、部分字段只在详情接口有）。

---

## 六 · 接入方自查（三条命令）

```bash
# ① 通不通
curl -sS "$ONTOK_BASE_URL/api/health"

# ② 有没有配方（应看到 10 个键）
curl -sS -X POST "$ONTOK_BASE_URL/api/sparql" \
  -H "Authorization: Bearer $ONTOK_TOKEN" -H "Content-Type: application/json" \
  -H 'Accept: application/ld+json' \
  -d '{"query":"SELECT ?c WHERE { ?c a :cvm_instance } LIMIT 1"}' \
  | python -c 'import json,sys; g=json.load(sys.stdin)["@pending"][0]["get"]; print(sorted(g))'
```

**③ 关键自检**：`get` 里的 `request.defaults` 有没有 `Region`；`alarm_policy` 有没有 `module: "monitor"`。

---

## 七 · 冻结期间的规则

| 允许 | 不允许 |
| --- | --- |
| 接入方联调、提问题 | 改本体（v3 已冻结，改就要 v4） |
| 我们改**文档 / skill**（不影响二进制） | 合并新需求后直接覆盖基线 |
| 发现问题 → 走新版本 | 热改 ConfigMap 里的 AM（会绕过签名审计） |

**发新版本的门槛**：接入方验收通过 → 我们合下一条需求 → 发 `v0.3.0` → 灰度切。

---

## 八 · 一句话

> **v0.2.0 之后，接入方不需要懂任何云厂商 API**：
> `get` 告诉你调什么、怎么拼、取哪个字段、怎么签名；
> 你只需要**照 `RECIPE.md` 剪枝、照 `AUTH-SPEC.md` 签名**。
