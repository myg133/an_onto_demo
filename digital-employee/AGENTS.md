# AGENTS.md · 数字员工接入 ontok 本体服务

> **这份文件是给"你"（数字员工 / Agent）的说明书**，不是给运维的。
> 通用流程的细节在 `skills/ontok-query/SKILL.md`；本文件补充**这套部署**的具体事实。

---

## 一 · 你是干什么的（一句话）

> 你拿到的是一个**本体服务**：它告诉你「**世界上有什么、怎么关联、怎么算**」，以及
> 「**哪些数据它没有，你该去哪个接口取**」。
> **它不替你取数，也不替你调云。**

```
你 ──问本体──▶ ontok 服务 ──▶ 「这些类是 API 源，按这个 get 去取」
  ◀────────────────────────────┘
你 ──按 get 调腾讯云 API──▶ 拿到真数据 ──▶ 与本体给的图合并 ──▶ 作答
```

---

## 二 · 连接（值由平台注入，⛔ 本文件不含地址与口令）

| 变量 | 用途 |
| --- | --- |
| `ONTOK_BASE_URL` | 服务根地址（例：`https://<你的部署域名>`） |
| `ONTOK_TOKEN` | **查询**口径的 token（不是管理口令） |

**MCP 配置**（推荐，`type` 必须是 `"http"`、`headers` 必须是对象、`url` 必须带 `/mcp`）：

```json
{ "mcpServers": { "ontok": {
    "type": "http",
    "url": "${ONTOK_BASE_URL}/mcp",
    "headers": { "Authorization": "Bearer ${ONTOK_TOKEN}" } } } }
```

**30 秒自测**（不依赖客户端）：

```bash
curl -X POST "$ONTOK_BASE_URL/mcp" \
  -H "Authorization: Bearer $ONTOK_TOKEN" -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

返回 4 个工具（`catalog` / `describe` / `sparql_query` / `sample`）⇒ 通了。

---

## 三 · 开工五步（**顺序不能乱**）

```
① health      → 弄清模式（决定能不能拿到数据）
② TTL         → 拿到地图（类/属性/关系/口径 + 哪些类来自 API）
③ catalog/describe → 定位概念，看 access.kind
④ sparql_query（JSON-LD）→ 能算的算完
⑤ 处理 @pending → 自己去取 → 合并 → 作答
```

### ① 先看模式

```bash
GET /api/health        # 或 MCP 的 initialize
```

| 字段 | 你要判断什么 |
| --- | --- |
| `mode` | `"ontology-only"` ⇒ **不接库**：库源类查不到，但 **api 类会给取数指引** |
| `project` / `ontology_version` | 你连的是哪套本体（版本对不上要提醒） |

### ② 取地图（**别跳过**）

```bash
GET /api/ontology.ttl        # 客户视图（推荐先读这个）
GET /api/ontology.ttl?view=model   # 建模视图（含 key/证据，判读用）
# MCP: resources/read ontok://ontology/customer.ttl
```

**为什么必须先取**：没有它你只能靠 `catalog` 一页页翻，而且不知道**类之间的关系**。

**TTL 里最要紧的一行**：

```turtle
:cvm_instance a owl:Class ; ontok:sourceKind "api" .   # ← 这类数据不在平台，要去外部接口取
```

### ③ 定位概念

```bash
GET /api/describe?name=cvm_instance
```

返回里有 **`access`**：

```json
{ "access": { "kind": "api", "queryable_by": ["fetch"],
              "pushdown_fields": ["region","zone"], "freshness": "realtime" } }
```

| `access.kind` | 你该走 |
| --- | --- |
| `db` | `sparql_query` |
| `api` | 查了会拿到 `@pending`，**按 `get` 自己去取** |

### ④ 查询（**默认就是 JSON-LD 图**）

```bash
POST /api/sparql
{ "query": "SELECT ?c WHERE { ?c a :cvm_instance } LIMIT 3" }
# 或 MCP: sparql_query { "query": "..." }   ← 不传 format 就是图
```

要表格才传 `format: "rows"`（`?format=rows` / `Accept: application/json`）。

### ⑤ 处理 `@pending`（**这一步决定你会不会答错**）

见第四节。

---

## 四 · `@pending` 怎么处理（**两条硬规矩**）

### 规矩 1：`meta.complete: false` ⇒ **不许直接作答**

```json
{ "@graph": [],
  "@pending": [ { "@type": "ontok:cvm_instance", "reason": "source_is_api",
      "get": { "class": "cvm_instance", "filter": {},
               "pushdown": ["region","zone"], "ref": "access#CvmInstance@v1" } } ],
  "meta": { "complete": false } }
```

**`complete: false` 的意思是「还缺一块」，不是「没有数据」。**

### 规矩 2：`filter: {}` ⇒ **不许无过滤全量拉**

`filter` 为空表示**平台无法替你确定取数标识**（这套部署没有库侧主键）。

| ⛔ 不要 | ✅ 要 |
| --- | --- |
| 直接调接口拉全量 | 先用该类的标识属性（`describe` 看，如 `:cvm_instance__instance_id`）**在你自己这侧**拿到标识 |
| 把 `pushdown` 当可选 | 暂时拿不到标识时，**至少用 `pushdown` 字段收窄**（如 `region` / `zone`） |

> **账单、告警一类接口全量拉 = 烧钱 + 触发限流。** 会被计费的。

### `get` 的四个字段

| 字段 | 含义 |
| --- | --- |
| `class` | 取哪个类（对应哪份**已签**映射） |
| `pushdown` | **只有这些能当请求参数**；其余条件只能拉回来自己筛 |
| `ref` | 指向已签映射版本（`access#<id>@v<版本>`）。调不通时报给运维，⛔ 别猜参数 |
| `filter` | 尽量照抄；为空时按规矩 2 |

### 取到之后

1. 用 `@id` / `@type` 与平台给的图**合并**（`@context` 里有 `ontok` 前缀）
2. 答案里**标出哪部分来自外部接口**
3. 再检查 `complete` 是否已能置真

---

## 四·五 · 取数侧（**你那一侧**）的四条铁律

> 这四条来自**一次真实巡检**的复盘：两轮共 58 次工具调用、15 次报错，**其中 6 次都出在下面这些地方**。

### ① 凭据只注入一次，⛔ 不要每条命令都重新贴

那轮实测里，同一条 `export SECRET_ID=…` **重复了 17 次**（每条 shell 都是新进程）。
后果：**真密钥进了会话日志**（导出成 HTML 就跟着走），还白烧 token。

```bash
# ✅ 写进 .env（chmod 600），之后只 source，⛔ 不要 echo
printf 'TC_SECRET_ID=%s\nTC_SECRET_KEY=%s\n' "$ID" "$KEY" > .env && chmod 600 .env
set -a; . ./.env; set +a
```

⛔ 禁：`echo $SECRET_KEY`、把密钥写进命令行参数（进 history）、把 `.env` 提交到仓库。

### ② ⛔ 不要从零造签名/翻页/地域发现

优先用**包里自带的执行器**（本部署：`ontok-builder access fetch`）；没有再自己写；
写就照 `skills/ontok-query/SKILL.md` **附录 A** 抄（那份已用腾讯云**官方向量**验过签名逐字节一致）。
⛔ **不要凭记忆写 version/参数**——实测有人把 SSL 接口猜成另一个版本（能跑通，但字段对不上本体）。

### ③ 路径用相对工作目录，⛔ 不要写 `/tmp`

实测失败：`curl -o /tmp/onto.ttl` → `系统找不到指定的文件`。用 `./x.ttl` 或 `./.tmp/x`。

### ④ 翻页形态因接口而异，⛔ 别假设只有一种

本部署就并存三种：顶层 `Offset/Limit`、**`PageNumber`/`PageSize`（页码从 1 起）**、
以及"`Offset` 与 `Limit` 都必填、`Limit` 有硬上限（账单是 300）"。

✅ 稳妥做法：**先取一页看有没有 `TotalCount`**；有就按它算页数，没有就取到空页为止。

### 附：本部署相关的两个具体坑

| 坑 | 事实 |
| --- | --- |
| COS 的 Python SDK 包名 | 是 **`cos-python-sdk-v5`**；⛔ 不是 `tencentcloud-sdk-python-cos`（不存在，实测装失败） |
| 其余产品 | 都是 `tencentcloud-sdk-python-<产品>`（cvm/cbs/cdb/clb/monitor/vpc/ssl/billing） |

---

## 五 · 跨源关系：**没人替你 join**

如果一条边两端一个在库、一个在 API：

```
那条边不会给你 get  ⇒  拆两段：先查库拿 ID，再按 ID 调接口
```

硬走会得到 `CROSS_SOURCE_UNSUPPORTED` —— **重试无效**。

---

## 六 · 错误 → 动作（**别把可重试和不可重试搞混**）

| code | 含义 | 你该做什么 |
| --- | --- | --- |
| `NO_DATA_SOURCE` | **需要库**的查询（库源类 / 混合 / `sample`） | ⛔ **别重试**；转本体回答，或改查 api 类 |
| `NOT_DB_SOURCE` | rows 模式查了 api 类 | 改要 JSON-LD（会拿到 `@pending`） |
| `CROSS_SOURCE_UNSUPPORTED` | 跨源遍历 | **拆两段**，⛔ 别重试 |
| `SPARQL_UNSUPPORTED` | 语法超出支持集 | 按 `hint` 改写 |
| `SPARQL_UNDEFINED_NAME` | 类/属性不存在 | 先查 `catalog` |
| **`RATE_LIMITED`**（你调云 API 时） | 触发限流 | ✅ **可以退避重试** |
| **`SCAN_LIMIT_EXCEEDED`**（你调云 API 时） | 扫太多 | ⛔ **重试无效**，要**改查询**（加更窄的过滤） |

> **最后两条是你自己那一侧的错**，别和平台的错混在一起。

---

## 七 · 禁忌

1. ⛔ **把 `complete: false` 当答案** —— 那是半成品
2. ⛔ **`filter: {}` 时全量拉** —— 会烧钱、会限流
3. ⛔ **拿 SPARQL 反复试 api 类** —— `access.kind` 已经告诉你了
4. ⛔ **自己拼租户条件** —— 租户由 token 绑定，服务端注入
5. ⛔ **在答案里省略 `warnings` / `coverage`** —— 那是"范围不对"的警告
6. ⛔ **把 `ref` 里的版本号当装饰** —— 它是你追溯"哪一版映射"的唯一凭据
7. ⛔ **对 `NO_DATA_SOURCE` 重试** —— 那不是暂时故障

---

## 八 · 这套部署的现状（问"为什么查不到"之前先看这里）

| 项 | 值 |
| --- | --- |
| `mode` | `ontology-only`（**不接业务库**） |
| 项目 / 版本 | `cloud-resource` / v3（已冻结） |
| 类 / 关系 / 属性 | **30 / 34 / 752** |
| **api 源类**（有取数契约） | **14 个**：`cvm_instance` `vpc` `subnet` `security_group` `security_group_rule` `cbs_disk` `cbs_snapshot` `clb_load_balancer` `clb_listener` `cdb_instance` `db_backup` `ssl_certificate` `alarm_policy` `cost_detail_bill` |
| 库源类 | **0 个**（这套没有库） |
| 其余类 | 有概念、**暂时没有取数契约** ⇒ 查了会 503，那是"还没接上"，不是"没有这个东西" |

---

## 九 · 一条完整示例

```
用户：广州这个区有没有异常的云服务器？

① health     → mode=ontology-only、project=cloud-resource v3
② TTL        → :cvm_instance 标了 ontok:sourceKind "api"
③ describe   → access.kind=api、pushdown_fields=[region,zone]
④ sparql（JSON-LD）
   SELECT ?c WHERE { ?c a :cvm_instance }
   ⇒ 200：@graph=[]、@pending=[1]、complete=false
      get = { class:"cvm_instance", filter:{}, pushdown:["region","zone"] }
⑤ 按规矩 2：filter 为空 ⇒ 先用 region/zone 收窄，再调腾讯云 DescribeInstances
   （分页、限流、签名都归你自己）
⑥ 拿到实例后：与 @pending 里的 @type 对齐，组装答案
⑦ 作答：
   「广州区共 3 台实例，其中 1 台 CPU 持续 >90%（数据来自腾讯云实时接口，
     本体 30 类里的 cvm_instance；本回答不含平台侧数据）」
```

---

## 十 · 一句话总结

> **平台负责"说清楚"，你负责"取回来"。**
> 它给的每一份 `@pending` 都是一张**填空题**：`class` 是要什么、`pushdown` 是能怎么筛、
> `ref` 是凭什么信它。**填完再作答，没填完就别答。**
