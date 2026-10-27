---
name: ontok-query
description: 用 ontok 本体服务回答业务问题。核心顺序是「先取本体（TTL）→ 定位概念 → 决定能否查数据 → 解读回执 → 转达限制」。含 v1 SPARQL 边界、两种部署模式的区别、每种错误的处置，以及 API 源的取数脚本（access_fetch.py，零依赖，别手写签名）。
category: scenario
---

# ontok-query · 用本体服务回答业务问题

你面对的不是一张表，而是一个**本体服务**：它把关系库翻译成「类 / 属性 / 关系」，并可能**在服务端强制注入租户过滤**。

> 本 skill 讲**流程**，不含任何具体业务名词。具体有哪些概念，取决于那套部署的本体——**你要先去取**。

---

## 〇 · 连接信息**由部署注入**（本文件不含地址与口令）

⛔ **这份 skill 里没有、也不许有** `base_url` 和 `token`。

| 为什么 | 说明 |
| --- | --- |
| skill 是**共享资产** | 会打包、分发、被复制到多个 Agent 目录 |
| 地址**每套部署一个** | UAT / PROD / 客户私有环境各不相同 |
| token 写进来 = **泄漏** | 进 git、进分发包、进日志截图 |

### 运行时从哪读

| # | 来源 | 变量/字段 |
| --- | --- | --- |
| 1 | **MCP server 配置**（推荐） | 平台侧 `mcpServers` 的 `url` + `headers` |
| 2 | 环境变量 | `ONTOK_BASE_URL`（如 `https://an-onto.example.com`）、`ONTOK_TOKEN` |

拿不到时 ⛔ **不要猜地址**（不要试 `localhost`、不要试上一套环境的 URL），直接说「当前环境没有注入 `ONTOK_BASE_URL` / `ONTOK_TOKEN`」。

> 猜地址最坏的结果不是报错，而是**连到别的环境**——那会把别人的数据当成本租户的答案。

### 这套服务**只有这些端点**（⛔ 别探别的）

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| GET | `/api/health` | 模式与版本（**第一步**） |
| GET | `/api/catalog?page=&page_size=` | 类 / 属性 / 关系（`kind` 区分） |
| GET | `/api/describe?name=` | 单个概念详情（含 `access.kind`） |
| GET | `/api/ontology.ttl`（`?view=model`） | 本体（**⛔ 大就别整份读**，见 §二） |
| POST | `/api/sparql`（`?format=jsonld\|rows`） | 查询 |
| POST/GET | `/mcp` | MCP（JSON-RPC） |

**⛔ 没有** `/openapi.json`、`/docs`、`/api/access`、`/api/mappings`——实测有 Agent 连探 8 次全部 404。
取数配方不在这套 HTTP 接口上：它在 `@pending[].get` 里（+ 找运维要已签映射，见 §4.4 ⑥）。

---

## 一 · 第 0 步：先弄清自己在**哪种模式**（决定你能做什么）

```
GET {BASE}/api/health        （免 token）
```

看响应里的 `mode`：

| `mode` | 这套部署能做什么 | 不能做什么 |
| --- | --- | --- |
| **没有该字段**（普通模式） | 查**真数据**（SPARQL）+ 本体 | —— |
| **`"ontology-only"`** | 本体（类/属性/关系/TTL）+ **api 源的取数指引（`@pending`）** | 库源类的数据：查它们才 `503 NO_DATA_SOURCE`（详见 §4.2） |

**这一步不能跳**。同样是"这个接口"，两种部署的能力边界完全不同。

---

## 二 · 第 1 步：**先取本体**（这是地图，没有它你只能瞎猜）

```
GET {BASE}/api/ontology.ttl            → 客户视图（默认）
GET {BASE}/api/ontology.ttl?view=model → 建模视图（若该部署允许）
```

MCP 用户也可以读资源：`ontok://ontology/customer.ttl`、`ontok://ontology/model.ttl`

### 为什么必须先取

> 本体的作用相当于**地图**：它告诉你**这套系统里有哪些概念、彼此什么关系、业务口径怎么算**。
> 没有地图，你连"该问什么"都不知道——只能拿用户的词去猜字段名，**猜错时不会报错，只会给出一个看起来对的答案**。

### 两个视图的区别

| 视图 | 有什么 | 什么时候用 |
| --- | --- | --- |
| **客户视图**（默认） | 类 / 属性 / 关系 / 口径；**不含**内部结构（表列名、租户列、主键） | 一般问答 |
| **建模视图**（`?view=model`） | 多了主键、枚举值域、租户列、**关系证据等级** | 需要判断"这条关系可不可信" |

### ⭐ TTL 里怎么认出「平台不会给我数据」的类

```turtle
:<类名> a owl:Class ;
    rdfs:label "<这个类的显示名>"@zh ;
    ontok:sourceKind "api" .        # ← 这个类的数据不在这套本体里，在外部接口
```

| 标记 | 含义 | 你该怎么做 |
| --- | --- | --- |
| 有 `ontok:sourceKind "api"` | 平台**不执行**这类数据源 | 查它只会拿到 `@pending`；要**用 `access_fetch.py` 取数**（§4.5） |
| **没有**这个谓词 | 库源（平台能查） | 正常写 SPARQL |

**写查询之前先扫一遍**——不然你会拿 SPARQL 去查 api 类，然后拿到空图还以为是"没数据"。

### 大本体怎么读（**⛔ 最常见的浪费**）

先看它多大：

```bash
curl -sS -H "Authorization: Bearer $TOK" "$ONTOK_BASE_URL/api/ontology.ttl" -o ./onto.ttl \
     -w 'bytes=%{size_download}\n'
```

| 大小 | 做法 |
| --- | --- |
| < 50 KB | 直接读 |
| **≥ 50 KB** | ⛔ **不要 `cat`**（实测：209 KB 的 TTL `cat` 一次，会话输入 token 直接多出十万级）；用下面三条命令**只取要的** |

```bash
# ① 有多少类、多少 api 源类
grep -c 'a owl:Class' onto.ttl
grep -c 'ontok:sourceKind "api"' onto.ttl

# ② 列出全部 api 源类（只要名字）
grep -B20 'ontok:sourceKind "api"' onto.ttl | grep -E '^:[A-Za-z_]' | sed 's/ .*//'

# ③ 看某一个类（含它的属性与关系），只取那一段
awk '/^:<类名> /{f=1} f&&/^\.$/{print;exit} f' onto.ttl
```

需要更多细节，用 `catalog` 分页 + `describe` 单个概念——**它们本来就是为"不全量读"设计的**。

> **一句话**：TTL 是**地图**，不是**正文**。看地图的方式是**查索引**，不是把整张纸塞进眼睛。

---

## 三 · 第 2 步：定位你要的概念

```
GET {BASE}/api/catalog?page=1&page_size=20
GET {BASE}/api/describe?name=<概念或属性名>
```

`catalog` 返回：`items`（`kind: class|property|relation`）+ `counts` + `total_items` + `has_more`
`describe` 返回：`display_name` / `key` / `properties[]` / `relations[]` / `tenant` / `primary_key`

**看 `counts` 就能知道本体规模**：`classes` / `properties` / `relations`。

> 有的部署会报 `excluded_relations`：那些是**本体里有、但这套部署查不了**的关系（通常是"没有可用的列映射"）。**别把它们当不存在**——先看 `describe` 里的 `evidence_level`。

---

## 四 · 第 3 步：判断"这个问题能不能用数据回答"

| 问题长相 | 本体模式 | 普通模式 |
| --- | --- | --- |
| 「有哪些概念 / 怎么关联 / 口径怎么算」 | ✅ 答 | ✅ 答 |
| 「有多少 / 多少钱 / 上个月趋势」 | ⛔ **只能说明没有数据源** | ✅ 查 SPARQL |
| 「要不要扩容 / 建议」 | ⛔ 本体不含决策规则 | ⛔ 同 |

### 4.1 普通模式：写 SPARQL（v1 边界，**实测结论**）

```sparql
SELECT ?x WHERE { ?x a :<某类> . ?x :<某属性> ?v } LIMIT 10
```

| ✅ 支持 | ⛔ 不支持（会**明确报错**，不是静默给空） |
| --- | --- |
| `SELECT` + `WHERE` 三元组、`FILTER`（比较/逻辑）、`LIMIT`、`ORDER BY`、`DISTINCT`、聚合（`COUNT`/`SUM`/`AVG`/`MIN`/`MAX` + `GROUP BY`） | `OPTIONAL`、`UNION`、子查询、属性路径（`/`、`*`、`+`）、`REGEX`、`SERVICE`、构造类（`CONSTRUCT`/`INSERT`） |


> ⭐ **默认回执是 JSON-LD 上下文子图**（`@context` / `@graph` / `summary` / `meta`），**没有** `columns` / `rows`。
> 要**表格**（旧的行回执）必须显式传：MCP `arguments.format = "rows"`；REST `?format=rows` 或 `Accept: application/json`。
> ⛔ **不传 `format` 拿到的是图**——别按表解析。

**不支持的写法不要绕**：报错里会给出 `code` 与 `hint`，照它改，或者诚实说这个接口做不到。

⛔ **不要在查询里写"哪个客户"**——租户由 token 绑定、服务端注入。带上别人的租户标识会被拒（这是正确行为）。

### 4.2 本体模式：**要分两种查询看**（这是最近才变的，别按老经验）

`mode = "ontology-only"` 表示**这套部署不接业务库**。但**不是所有查询都 503**：

| 你查的类 | 结果 | 你该做什么 |
| --- | --- | --- |
| **库源类**（`access.kind = "db"`） | `503 NO_DATA_SOURCE` | **重试无效**。转成本体回答：「这里没有数据，只有概念与口径」 |
| **API 源类**（`access.kind = "api"`） | **200 + `@graph: []` + `@pending` + `complete: false`** | ✅ **用 `access_fetch.py` 取数**（§4.5；`get` 的字段含义见 §4.3） |
| **混合**（库类 + api 类） | `503` | 它确实需要库 ⇒ 拆开：先问本体、再按 `get` 取 |

**怎么提前知道查的是哪类**：`describe` / `catalog` 的 `access.kind`（`db` 走 SPARQL、`api` 走 fetch）。
或者直接看 TTL 里这个类有没有 `ontok:sourceKind "api"`。

⛔ **不要因为"这套是本体模式"就放弃 `@pending`**——那正是它能给的最有价值的东西。

---

### 4.3 API 源：平台只给「取数指引」，数据**用脚本取**

> ✅ **这一节只讲「指引长什么样」；真正取数用 `access_fetch.py`，命令与错误码见 §4.5。**
> ⛔ **不要照 `AUTH-SPEC.md` 自己实现 TC3 签名**——那份规格是脚本的来源，不是让你再抄一遍。

对 `access.kind = "api"` 的类，平台返回的是**半成品**：

```json
{ "@graph": [],
  "@pending": [
    { "@type": "ontok:<class>", "reason": "source_is_api",
      "get": { "class": "<class>",
               "filter": {},                       // ← 注意这一项，见下
               "pushdown": ["<可下推参数>", "<另一个>"],
               "ref": "access#<entity id>@v<版本>" } } ],
  "meta": { "complete": false, "unresolved_classes": ["<class>"] } }
```

| `get` 里的字段 | 你怎么用 |
| --- | --- |
| `class` | 要取哪个类（对应哪份已签映射） |
| `pushdown` | **只有这些字段能当请求参数**；其余条件只能拉回来自己筛 |
| `ref` | 指向**已签**的映射版本（`access#<id>@v<版本>`）。调不通时报给运维，别自己猜参数 |
| **`filter`** | **可能为空 `{}`** ⚠️ 见下面两条 |

**⚠️ `filter: {}` 是危险的**——它意味着**平台无法替你确定取数标识**（这套部署没有库侧主键）。

| ⛔ 不要 | ✅ 要 |
| --- | --- |
| 直接**无过滤全量拉取** | 先用该类的标识属性（`describe` 看，如 `:<class>__<标识属性>`）**在你自己的取数侧**拿到标识，再带 `ids` 去取 |
| 把 `pushdown` 当"随便筛" | 暂时拿不到标识时，**至少用 `pushdown` 里的字段收窄**范围 |

> **账单、告警一类接口全量拉 = 烧钱 + 触发限流。** 这不是保守，是会被计费的。

**取到之后**：

1. 用 `@id` / `@type` 与平台给的图**合并**（`@context` 里有 `ontok` 前缀）
2. 答案里**标出**哪部分来自外部接口
3. `meta.complete` 现在是 `false` —— 处理完 `@pending` 再作答

**跨源边**：如果两个类一个在库、一个在 API，那条边**不会给你 `get`** ⇒ 拆两段：先查库拿 ID，再按 ID 调接口。
硬走会得到 `CROSS_SOURCE_UNSUPPORTED`——**重试无效**。

### 4.4 取数侧怎么落地（**实测踩过的坑**）

拿到 `get` 之后，取数由**脚本**完成（§4.5）。下面几条是「把脚本用对」的经验，⛔ 不是让你照着自己实现一遍。

#### ① 凭据**只注入一次**，⛔ 不要每次调用都重新贴

实测最坏的一轮：同一条 `export SECRET_ID=… SECRET_KEY=…` 在 47 次 shell 调用里**重复了 17 次**。
后果：**凭据明文进了会话日志**（会话导出会带着它走），而且白白烧 token。

✅ 正确做法（按环境挑一个）：

```bash
# A. 写进 .env，之后只 source（推荐）
printf 'TC_SECRET_ID=%s\nTC_SECRET_KEY=%s\n' "$ID" "$KEY" > .env && chmod 600 .env
set -a; . ./.env; set +a     # 每条命令前 source 一次，⛔ 不要 echo 变量

# B. 包里自带执行器（若有，最省事：签名/翻页/限流都替你做了）
#    例：ontok access fetch --mapping <已签映射> --entity <class> --filter '{...}' --allow-network
```

⛔ 禁忌：`echo $SECRET_KEY`、把凭据写进命令行参数（会进 shell history 与进程列表）、
把凭据写进脚本文件后提交。

#### ② ⛔ 不要自己从零造签名 / 翻页 / 地域发现

实测一轮里 9 次错误，**6 次**都在这上面（模块名猜错、并发代码写崩、地域列表没有）。

✅ 正确姿势：**调用 `access_fetch.py`**（§4.5）。签名、分页、剪枝、字段映射、10 个固定错误码（9 个既有 + `auth.unsupported`），它都实现好了。
只有当包里**没有**这个脚本（例如客户环境根本没有 Python）才考虑手写，那时照附录 A 抄，⛔ 别重新发明。

#### ③ 路径用**相对工作目录**，⛔ 不要写 `/tmp`；⛔ 也别每条命令都 `cd`

实测：`curl -o /tmp/<文件>` 在 Windows 上直接失败（`系统找不到指定的文件`）。
✅ 用 `./onto.ttl` 或 `./.tmp/<name>`，结束时清掉临时目录。

另一个实测：**56 条命令里有 56 条**以 `cd <某个本机绝对路径>;` 开头——每条 shell 都是新进程，
所以要重来一遍。这不是错，但纯浪费。

✅ 二选一：
- **用绝对路径**（`python <脚本全路径> fetch …`），一次到位；
- 或者把 `cd` 写进**入口脚本**里，命令只调脚本（见 ⑤）。

#### ④ 翻页形态**因接口而异**，⛔ 不要假设一种就够

同一家云里就并存三种：顶层 `Offset/Limit`、`PageNumber/PageSize`（页码从 **1** 起）、
以及"`Offset` 与 `Limit` 都必填 + `Limit` 有硬上限（如 300）"。

✅ 做法：**先取一页，看返回里有没有 `TotalCount`**；有 `TotalCount` 就按它算页数，
没有就"取到空页为止"。⛔ 不要写死 `while offset < 1000` 这种假设。

#### ⑤ 取数**不要写脚本**，分析**要写成文件**

实测最坏的一轮：47 次 bash 调用里，**大量是重复定义同一套 helper**（写一次 `tc.py`，却几乎没复用它）。

✅ 正确姿势：

```bash
# 取数：用现成的（签名 / 翻页 / 剪枝 / 错误码都在里面，见 §4.5）
python access_fetch.py --mapping <am.json> --entity <entity id> --param <名>=<值> --out ./rows.json
python access_fetch.py --mapping <am.json> --entity <另一个 entity id> --param <名>=<值> --out ./rows2.json
```

**分析照样要落成文件**（实测：分析阶段 25 次 `python - <<'PY'`，每次重导 `json`/`collections`）：

```bash
# 一次写好，按"问题"分子命令
python ana.py policies      # 告警策略：启用但零绑定
python ana.py certs         # 证书：过期 / 临期
python ana.py bill --class <class>
```

| 反例 | 为什么坏 |
| --- | --- |
| 每次 `python - <<'EOF'` 重写 helper | 每次都要重新解释一遍逻辑；错一次就是一次工具调用；日志里全是重复代码 |
| 把 helper 写在会话里、靠"记得" | 上下文一长就记错，改一次要重发全部 |

> **判断标准**：同一种"取数/解析"要做第 2 次时，就该抽成文件；⛔ 不要复制粘贴改三个字符。

#### ⑦ 配套文档与脚本（**遇到细节就翻这些，别猜**）

| 文件 | 讲什么 |
| --- | --- |
| [`access_fetch.py`](access_fetch.py) | **取数脚本本体**（零依赖）。怎么跑见 §4.5 —— 这一份就够了，⛔ 不用自己写 |
| [`AM-FIELDS.md`](AM-FIELDS.md) | AM 每个键是什么、脚本用了哪些、哪些不用；改映射之前先看它 |
| [`AUTH-SPEC.md`](AUTH-SPEC.md) | 签名算法：规范化、时间戳、scope、签名字符串、`Authorization` 拼法、常见坑（**规格**，脚本已实现） |
| [`RECIPE.md`](RECIPE.md) | `get` 的 10 个键怎么用：占位符来源、**剪枝四规则**、分页 `value_type`、`required` 语义、`iterator`/`fields` |

> ⚠️ 后两份是**给客户端实现者的规格**。你只要会**调用脚本**（§4.5）就能接上；
> 只有在脚本不可用时才需要读它们、自己实现。

#### ⑥ ⛔ 不要"找接口规格"，也不要翻 SDK 源码

实测（同一轮里出现了三次）：

| 它干的 | 为什么白干 |
| --- | --- |
| `curl /openapi.json`、`/api/access`、`/docs` | **这些端点不存在** ⇒ 404。可用端点见 §〇 的表 |
| `grep -n '<类名>' site-packages/<某厂商 SDK>/...` | 翻 SDK 源码猜参数名 |
| 猜接口版本（实测有人把对象的版本猜成另一个） | 能跑通，但**取回的字段可能对不上本体** |

**正确顺序**：

1. 参数与版本优先从 **`get`** 拿（`pushdown` / `operation` / `request`——本部署正在补齐）
2. 拿不到 ⇒ **找运维要那份已签映射**（`ref` 就是它的编号），然后交给 `access_fetch.py`（§4.5）
3. ⛔ 最后才考虑翻文档；**⛔ 不要翻 SDK 源码，⛔ 不要探端点**

---

### 4.5 API 源取数：**用脚本，别手写**

> **一句话**：⛔ **不要照 `AUTH-SPEC.md` 自己实现 TC3 签名、翻页、剪枝。**
> 直接调用 `access_fetch.py`——它就是那份参考实现，零第三方依赖（Python 3.8+ 标准库）。
>
> 手写一遍的代价是实测过的：签名不匹配、时间戳用了本地日期、分页死循环、字段路径写错，
> 9 次错误里 6 次出在这几件事上（§4.4②）。

#### ① 脚本在哪、怎么调

`access_fetch.py` **与本文件同目录**。调用就是文件名本身：

```bash
python access_fetch.py --selftest
```

**本文不规定任何目录**——脚本放在哪、`--mapping` 指向哪个 AM、`--out` 写到哪，
**全由调用方（你的运行环境）决定**：

| 参数 | 由谁给 |
| --- | --- |
| `--mapping` | 调用方给的 AM 文件（**本 skill 不带、也不需要知道它在哪**） |
| `--out` | 你自己的输出位置 |
| 脚本自身的路径 | 你的运行环境（agent 宿主）负责把 skill 文件放到位 |

> ⛔ **不要去搜索、猜测或追问任何绝对路径 / 样例目录**。本 skill 里**没有任何**机器相关的路径；
> 出现"目录找不到"时，正确反应是**问调用方要 AM 路径**，⛔ 不是去翻文件系统找某个目录名。
>
> 唯一需要跑一次的是离线自检：`python access_fetch.py --selftest`（不需要 AM、不需要凭据、不需要网络）。

#### ② 最常用的命令

```bash
# 0) 先跑一次「不需要 AM、不需要凭据」的离线自检（断网也能过）
$FETCH --selftest

# 0.5) 看当前支持哪些 auth.mode（AM 里只能写这里列出来的）
$FETCH --list-signers

# 1) 体检 AM：结构对不对、entity 签没签、夹具在不在。不取数、不要凭据
$FETCH --mapping <am.json> --check

# 2) 试跑：用 entity.fixture 走完整流程，不发网络、不要凭据
$FETCH --mapping <am.json> --entity <entity id> --dry-run

# 3) 真取数：凭据只从 auth.credential_env 命名的环境变量读
export <变量名1>=<值>  <变量名2>=<值>    # 变量名照 AM 的 auth.credential_env 写；只 export 一次，⛔ 别 echo
$FETCH --mapping <am.json> --entity <entity id> \
    --param <名>=<值> --max-rows 500 --out ./rows.json
```

参数就这些（`--param` 可重复，**只允许 `capability.pushdown` 里的键**）：

| 选项 | 作用 |
| --- | --- |
| `--mapping <am.json>` | 已签的 access mapping（`--check` / 取数必填） |
| `--entity <id\|class>` | 取哪个实体（entity 的 `id` 或本体 `class` 都行） |
| `--param K=V` | 下推参数；不在 `pushdown` 里会被拒，并**列出可用参数** |
| `--limit N` | 每页条数（会被 `paging.max_limit` 夹住） |
| `--max-rows N` | 最多取多少条（默认 1000；**取不全要如实说**） |
| `--dry-run` | 用夹具，不发网络 |
| `--check` | 只体检 AM，不取数 |
| `--list-signers` | 打印已支持的 `auth.mode`（AM 里只能写这些） |
| `--format jsonld\|rows` | 默认 JSON-LD；要平表用 `rows` |
| `--out FILE` | 写文件（默认 stdout） |
| `--selftest` | 离线自检（无网络、无凭据） |

输出形状（`meta` 是给你判断「够不够」的，**必须读**）：

```json
{ "@context": { "ontok": "https://ontok.dev/ns#" },
  "@graph": [
    { "@id": "ontok:<class>/<id>", "@type": "ontok:<class>",
      "ontok:<属性>": "<值>", "ontok:<另一个属性>": 4 } ],
  "meta": { "entity": "<entity id>", "class": "<class>",
            "rows": 1, "pages": 1, "truncated": false,
            "source": "<source id>", "mapping_version": <版本> } }
```

> 一份填了具体厂商值的同名样例在**附录 B**（标了「示例不是规格」）：形状一样，名字只是那一家的。

#### ③ 两条铁律（最容易踩）

| 铁律 | 说明 |
| --- | --- |
| **未签的 entity 取不了** | `reviewed_by` 为空 ⇒ `[am.unsigned] …还没签`，退出码 **2**，一个请求都不发（与 R6 同一条纪律）。**别绕**：让映射作者补 `reviewed_by`（`name@YYYY-MM-DD`），或换一个已签的 entity（`--check` 会列出哪个没签） |
| **截断时退出码是 8** | `meta.truncated: true` = **没取全**（到 `--max-rows`，或到 1000 页保险丝）。它不是一个警告，是把「数据不完整」写进契约。⛔ 别把 `truncated=true` 当"取全了"去作答：调大 `--max-rows` 或收窄 `--param`，实在取不全就在答案里说「这是前 N 条」 |

#### ④ 常见错误表（10 个码：你看到它说明什么、怎么修）

| 退出码 | 码 | 你看到它说明什么 | 怎么修 |
| --- | --- | --- | --- |
| **2** | `am.invalid` | AM 结构/版本/必填键不对（含 `auth.service` 缺失），或 `request.required` 里的名字这次没给值 | 看消息点名的那一处：`contract_version` 只认 `1` / `2`（别的值消息里会列出支持列表）；缺键补键；缺业务标识就补 `--param`（例如 `--param <名>=<值>`）。改完先 `--check` |
| **2** | `am.unsigned` | 这个 entity 的 `reviewed_by` 为空——**未签不许取数** | 找映射作者签（`name@YYYY-MM-DD`），或换已签的 entity。⛔ 不要自己填个名字糊过去 |
| **2** | `param.not_pushdown` | 你给的 `--param` 不在 `capability.pushdown` 里 | 用消息里列出的可用参数；其余条件只能拉回来自己筛（或在 AM 里加 pushdown 声明） |
| **3** | `credential.missing` | `auth.credential_env` 命名的环境变量没设 | 在当前 shell export 一次（消息里会点名变量）；⛔ 不要把值写进命令行/脚本/AM |
| **4** | `auth.rejected` | 401/403，或签名不匹配（服务端判签名错） | 先确认凭据有效、机器时钟准（时间戳是 **UTC 秒**）；再确认 AM 的 `auth.*`（含 `auth.service`）与实际发的 header 一致。⛔ 不要自己重写签名 |
| **4** | `auth.unsupported` | `auth.mode` 没有对应的签名器（⛔ 脚本**不会**退回别的算法） | 用消息里列出的已支持 mode（`--list-signers` 可查）；确实要这个算法 ⇒ 提需求加签名器（加一个不用改主流程） |
| **5** | `api.throttled` | 429 / 被限流（服务端说请求太多） | 降 `--limit` / `--max-rows`，收窄 `--param` 范围，隔一会儿再来。脚本**不自动退避**，重试由你决定 |
| **6** | `api.invalid` | 400 / 服务端说参数不对（未知参数、参数值不合法） | 看 `--dry-run` 打出的 `meta.request_body`；有的参数只能走 header，⛔ 别两处都写（RECIPE §七①） |
| **7** | `net.timeout` / `net.error` | 超时 / 连不上 / 5xx | 查 DNS、代理、出网策略；5xx 稍后重试。⛔ 不要改成猜地址（会连到别的环境） |
| **8** | `rows.truncated` | 到 `--max-rows`（或分页保险丝）还没取全 | **数据可用但没取全**：调大 `--max-rows`、收窄 `--param`；答题时说清范围。⛔ 别当取全了 |

> `--check` 就红（退出 2）时：先修 AM，⛔ 别急着取数——那是白跑一次请求，还可能计费。

#### ⑤ 规格还在，但不再是唯一交付物

| 文件 | 现在该怎么用 |
| --- | --- |
| [`AUTH-SPEC.md`](AUTH-SPEC.md) | 签名算法**规格**。脚本已按它实现（对官方 §7 向量逐字节相等）；只有你要读算法、换厂商、或怀疑脚本时才看 |
| [`RECIPE.md`](RECIPE.md) | `get` 的 10 个键、剪枝四规则、分页 `value_type`、`required` 语义。脚本已实现 |
| [`AM-FIELDS.md`](AM-FIELDS.md) | AM 每个键是什么、脚本用了哪些、哪些不用。**改映射前必看** |

> 脚本的行为就是这两份规格的实现。**两者冲突时以脚本为准**，并把「哪条规格写错了」报上来——别各自解释。

#### ⑥ 脚本**不做**的事（别指望）

| 不做 | 原因 |
| --- | --- |
| 自动重试 / 指数退避 | v1 只负责「把错报清楚」，重试策略交给调用方 |
| 用 `local_only` 帮你本地筛 | 返回的是**原始行**；筛了却不说是更危险的错 |
| 把结果推回服务端 | 行只在你手上（服务端不存这些行） |
| 支持 `mysql` 等别的源类型 | v1 只做 `http-json` |

---

### 4.6 AM 契约：数字员工可以依赖什么

> AM（access mapping，**访问映射**）是**本体 ↔ 取数方式的声明式映射**：某个 `<class>` 该去哪个
> `<source>`、用哪个接口、参数怎么拼、结果怎么切成一行、字段怎么落成属性。
>
> 本节讲**你能依赖什么、违反了会怎样**；逐键说明只有一处：[`AM-FIELDS.md`](AM-FIELDS.md)。
> ⛔ 本节不重复字段表 —— 两处各写一份，一定会漂。

#### ① AM 是什么：一份**人签过**的声明

- 三段结构：`sources[]`（怎么连、怎么签）、`entities[]`（哪个 `<class>` 取哪个接口、怎么切行）、`relations[]`（脚本不读）。
- **它由人签**：每个 entity 的 `reviewed_by` 是签名（`name@YYYY-MM-DD`）。签了才算契约。
- 为什么这么设计：映射写错**不会报错**，只会给出一个「看起来对」的答案。签名把「谁为这份映射负责」写进文件，出了事有人可找。

#### ② 三个版本号：谁决定「能不能执行」

| 版本号 | 管什么 | 脚本怎么用它 |
| --- | --- | --- |
| `contract_version` | **契约形状**（有哪些键、键是什么意思、哪些必填） | **白名单**：`1` / `2`（对照见下）。不在名单 ⇒ `am.invalid`（退出 2），消息**列出支持的版本**，⛔ 不「尽力解析」 |
| `mapping_version` | 映射**内容**的修订号 | 不判断，原样抄进输出 `meta.mapping_version` 供追溯 |
| `ontology_version` | 这份映射绑的是哪版本体 | 不判断（`--check` 只打印，不拦） |

**不一致时谁说了算**：能不能执行**只看** `contract_version`；`mapping_version` / `ontology_version`
不一致**不拦**，但必须能在回执的 `meta` 里看见。

**版本对照**（`--check` 每次都会打印「识别到哪一版 + 该版本下脚本读了哪些 `auth` 键」）：

| 版本 | 是什么 · 脚本怎么对待 |
| --- | --- |
| `1` | **原始契约**：`auth` 只声明 `mode` + `credential_env`；`signed_headers` / `timestamp` / `scope` / `content_type` 由**签名器默认值**兜底（`--check` 末尾会提示正在用默认值）。 |
| `2` | **三条契约增量，全部可选**（⇒ v1 映射照常加载、行为逐字节不变）：① `operation` 泛化成 `headers{}` + `vars{}`（厂商概念降级成 header 模板，字段名里不再出现厂商名词）；② `auth` 可显式声明 `signed_headers` / `timestamp` / `scope`（+ `content_type`）；③ `@pending.get` 允许给**完整取数配方**（v1 只认 4 键 `class` / `filter` / `pushdown` / `ref`，一个不多）。⚠️ 数据侧差异（**不是契约差异**）：v2 的数据里用上了 `operation.path` / `.headers` / `.vars`，并把 `Region` 从 `request.template` 挪进 `operation.headers` 的 `{region}` 占位符；**脚本对两版一视同仁**，这几个键两版都读、缺了按兜底（见 [`AM-FIELDS.md`](AM-FIELDS.md) §3.2）。 |
| `3` 及以上 / 非整数 | ⛔ **一律拒绝**：`am.invalid`（退出 2），消息**列出支持的版本**（`1, 2`）。⛔ 不「尽力解析」、不按最新版硬读。 |

三条最容易误读的点：

1. **脚本读哪些键，与版本无关**：两版读的是同一组 `auth` 键 —— `mode` · `credential_env` · `service` 必填
   （`service` 由签名器要求显式），`signed_headers` · `timestamp` · `scope` · `content_type` 可缺（缺则签名器默认）。
   版本只决定「契约**期望**你写不写」，⛔ **不要因为 AM 是 v2 就在调用侧加分支**。
2. **显式写 ≠ 改签名**：v2 把四件套写进 AM 后，签出的 `Authorization` 必须与走签名器默认值时**逐字节相同**
   （`--selftest` 用同一签名向量对拍：同一份 `Signature`，否则就是「换个写法把签名签坏了」）。
3. **v2 是「允许写」，不是「必须写」**：v2 映射里缺某个新字段 ⇒ 那一项**就是不出现**，
   ⛔ **不要按 `auth.mode` 猜一个默认值**。源码注释说得很直接：
   「**猜出来的签名头是错的，而错签名比没签名更难查**」。
   ⇒ 缺 `auth.service`（`tc3` 的必填项）时脚本**直接拒**（`am.invalid`），不会替你补。


#### ③ 必填 vs 可选

- **必填**（缺 ⇒ `am.invalid`，退出 2）：`contract_version`、`sources[].{id,kind,base_url,auth}`、
  `auth.mode`、`auth.credential_env`（两个变量名）、**签名器要求显式声明的键**（`tc3-hmac-sha256` 目前是 `auth.service`）、
  `entities[].{id,class,source,operation,iri_template}` 以及 `reviewed_by`（缺它 ⇒ `am.unsigned`，退出 2）。
- **缺了用签名器默认值**（`--check` 会提示）：`auth.signed_headers` / `auth.timestamp` / `auth.scope`。
- **每个键到底是什么、脚本读不读**：只看 [`AM-FIELDS.md`](AM-FIELDS.md)（**唯一一份**字段表）。

#### ④ 不变量（消费方可以直接依赖）

| # | 不变量 | 违反了会怎样 |
| --- | --- | --- |
| 1 | `entities[].source` 必须出现在 `sources[].id` 里 | `am.invalid`（2），消息列出已有 id |
| 2 | `iterator` 落在支持子集：`$.a.b[*]` / `$.a.b` | 别的写法 ⇒ `am.invalid`（2）—— ⛔ 脚本不猜路径 |
| 3 | `fields.<属性>.path` 是**记录内**的点号路径（不支持数组下标） | 写法越界 ⇒ `am.invalid`（2）；某行缺这条路径只是「这行没这个属性」，**不丢整行** |
| 4 | `iri_template` 的 `{id}` 由 `identity.key_path` 填 | 模板缺 `{id}` ⇒ `am.invalid`（2）；某行取不到 key ⇒ 该行跳过并计入 `meta.skipped_no_identity`（不静默） |
| 5 | `capability.pushdown` 是**允许参数的白名单** | 传了白名单外的 `--param` ⇒ `param.not_pushdown`（2），并列出可用参数 |
| 6 | `paging` 为空 ⇒ **单次取数**（不是「默认分页」） | 没声明分页就不翻页；`meta.truncated` 只在真没取全时才是 `true` |

#### ⑤ 语义边界（最容易被误读的四条）

| 你会这么以为 | 实际上 |
| --- | --- |
| `freshness` 保证数据是新的 | 它是**声明**：这个源正常情况下多新。⛔ 不是对本次回执的保证 |
| 取数就该有数据 | AM 只声明「怎么取」，**不保证这次有数据**。`@pending`、空 `@graph`、`rows: []` 都是**合法回执** |
| 空结果 = 没权限 | 不是。没权限是 `auth.rejected`（4）；空就是空 |
| `truncated: false` = 一定是全量 | 是「按服务端给的总数 / 短页判断取全了」；服务端不报总数时，判断依据就是短页 |

#### ⑥ 签署纪律

- `reviewed_by` 为空 ⇒ **拒取**：`am.unsigned` + 退出码 **2**，**一个请求都不发**。
- ⛔ 「先生成后补签」不是可用路径：脚本挡的是**使用时刻**的签名状态；空着就是未签。
- ⛔ 不要自己填个名字糊过去 —— 那是伪造签名，比未签更糟。

#### ⑦ 契约怎么演进

| 改动 | 怎么办 |
| --- | --- |
| 改**契约形状**（加必填键、改键义、改不变量） | 升 `contract_version` 并通知所有消费方；旧脚本遇到不认识的值 ⇒ `am.invalid`（2），⛔ 不许「尽力解析」 |
| 只改**映射内容**（换个参数名、改条路径、补签署人） | 升 `mapping_version`，`contract_version` 不动 |

#### ⑧ 错误码 ↔ 契约里被违反的是哪一条

| 码 | 退出 | 对应哪条 |
| --- | --- | --- |
| `am.invalid` | 2 | ②形状/版本、③必填键、④-2/3/4 路径子集、`auth.service` 缺失 |
| `am.unsigned` | 2 | ⑥ 签署纪律（`reviewed_by` 空） |
| `param.not_pushdown` | 2 | ④-5 pushdown 白名单 |
| `credential.missing` | 3 | `auth.credential_env` 命名的环境变量没设 |
| `auth.unsupported` | 4 | `auth.mode` 没有对应签名器（⛔ 不 fallback） |
| `auth.rejected` | 4 | 凭据 / 签名 / 权限被服务端拒 |
| `api.throttled` | 5 | 限流（运行时状态，与契约无关） |
| `api.invalid` | 6 | 服务端不接受这次请求的参数 |
| `net.timeout` / `net.error` | 7 | 网络（与契约无关） |
| `rows.truncated` | 8 | ④-6 / `--max-rows`：数据没取全 |

> 码与退出码的**唯一一份**表在脚本的 `CODE_EXIT`；行为与本文冲突时**以脚本为准**，并把文档改对。

---

## 附录 A · 参考实现（**旧·仅当拿不到 `access_fetch.py` 时**）

> **示例不是规格。** 本附录（以及附录 B）里出现的具体厂商名、接口名、字段名只是**那一家的样子**，
> 用来把「逐字节正确的签名」讲清楚；它们**不是** skill 的规定，换一家就换掉。
>
> ⛔ **先看 §4.5**：正常路径是调用 `access_fetch.py`，本附录只在「包里没有脚本且环境有 Python」
> 时作为最后兜底，且**不推荐**——它没有错误码、没有剪枝、没有截断标记，正是 §4.4② 说过的坑。
>
> ⚠️ **为什么附录里出现具体厂商**：这套部署的 `get` 指向某家云，签名算法必须逐字节正确，
> 抽象描述不足以实现。**换厂商只需替换 `call()` 里的签名段**，其余（翻页/重试/收窄）通用。
>
> 用途：当包里**没有**现成执行器时，抄这段，⛔ 不要自己重写。

```python
# ref_fetch.py —— 不要 echo 凭据；从环境变量读
import os, json, time, hmac, hashlib, urllib.request

SID, KEY = os.environ["TC_SECRET_ID"], os.environ["TC_SECRET_KEY"]

def call(host, service, action, version, region, payload, timeout=60):
    """一次 POST；返回 Response 里的 dict。签名 = TC3-HMAC-SHA256。"""
    body = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
    ts = int(time.time())
    date = time.strftime("%Y-%m-%d", time.gmtime(ts))

    # —— 签名段（换厂商只需改这里）——
    signed = f"content-type;host;x-tc-action"
    canon_headers = (f"content-type:application/json; charset=utf-8\n"
                     f"host:{host}\nx-tc-action:{action.lower()}\n")
    canon_req = "\n".join(["POST", "/", "", canon_headers, signed,
                           hashlib.sha256(body.encode()).hexdigest()])
    scope = f"{date}/{service}/tc3_request"
    sts = "\n".join(["TC3-HMAC-SHA256", str(ts), scope,
                     hashlib.sha256(canon_req.encode()).hexdigest()])
    k = ("TC3" + KEY).encode()
    for msg in (date, service, "tc3_request"):
        k = hmac.new(k, msg.encode(), hashlib.sha256).digest()
    sig = hmac.new(k, sts.encode(), hashlib.sha256).hexdigest()
    auth = (f"TC3-HMAC-SHA256 Credential={SID}/{scope}, "
            f"SignedHeaders={signed}, Signature={sig}")

    headers = {"Authorization": auth, "Content-Type": "application/json; charset=utf-8",
               "Host": host, "X-TC-Action": action, "X-TC-Version": version,
               "X-TC-Timestamp": str(ts)}
    if region:
        headers["X-TC-Region"] = region
    req = urllib.request.Request(f"https://{host}/", body.encode(), headers, method="POST")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        out = json.loads(r.read())["Response"]
    if "Error" in out:                       # 厂商把错误放在 200 里
        raise RuntimeError(f'{action}: {out["Error"]["Code"]} {out["Error"]["Message"]}')
    return out

def pages(host, service, action, version, region, payload, list_key,
          offset_param="Offset", limit_param="Limit", limit=100, cap=50):
    """翻页：有 TotalCount 按它算，没有就取到空页。⛔ 不写死上限假设。"""
    got, page = [], 0
    while page < cap:
        p = dict(payload); p[offset_param] = page * limit; p[limit_param] = limit
        out = call(host, service, action, version, region, p)
        batch = out.get(list_key) or []
        got += batch
        total = out.get("TotalCount")
        page += 1
        if not batch or (isinstance(total, int) and len(got) >= total):
            break
    return got

if __name__ == "__main__":
    # 地域别硬编码：先问一次
    host = "cvm.tencentcloudapi.com"
    regs = call(host, "cvm", "DescribeRegions", "2017-03-12", "", {})["RegionSet"]
    for rg in [r["Region"] for r in regs]:
        try:
            rows = pages(host, "cvm", "DescribeInstances", "2017-03-12", rg, {}, "InstanceSet")
            print(rg, len(rows))             # 逐地域收窄，⛔ 不无过滤全量拉
        except Exception as e:
            print(rg, "ERR", e)
```

**注意**：示例里的 `host/action/version` **必须来自 `get`（或 `describe` 的 `access`），不要凭记忆写**。
若 `get` 没给，就按 `ref` 找运维要**已签映射**——⛔ 不要猜版本（实测有人把 SSL 猜成另一个版本，能跑通但字段对不上本体）。

---

## 附录 B · 示例（某云厂商，仅供参考）

> **示例不是规格。** 这一节只是把 §4.5 的占位符填成**某一家云**的样子，方便第一次用的人对照；
> 名字（`CvmInstance` / `tencent-cvm` / `X-TC-*` / `TENCENT_*`）都属于**那一家的 AM**，不是本 skill 的规定。
> 换成别的厂商/别的本体时，命令里的 `<entity id>`、参数名、环境变量名都按**你拿到的 AM** 填。

**命令的填法**（对应 §4.5 ② 的命令示例）：

```bash
python access_fetch.py --mapping am.json --entity CvmInstance --dry-run
python access_fetch.py --mapping am.json --entity CvmInstance --param region=ap-guangzhou --out ./cvm.json
export TENCENT_SECRET_ID=...  TENCENT_SECRET_KEY=...     # 变量名来自 AM 的 auth.credential_env
```

**输出形状**（对应 §4.5 ② 的 JSON；`meta` 字段一个不少）：

```json
{ "@context": { "ontok": "https://ontok.dev/ns#" },
  "@graph": [
    { "@id": "ontok:cvm_instance/ins-abc123", "@type": "ontok:cvm_instance",
      "ontok:instance_id": "ins-abc123", "ontok:cpu": 4 } ],
  "meta": { "entity": "CvmInstance", "class": "cvm_instance",
            "rows": 1, "pages": 1, "truncated": false,
            "source": "tencent-cvm", "mapping_version": 1 } }
```

**错误消息长什么样**（对应 §4.5 ④；码与退出码是规格，具体串是示例）：

| 你会看到 | 这一家云里的样子 |
| --- | --- |
| 签名被拒（`auth.rejected`，4） | `AuthFailure.SignatureFailure`：时钟、`auth.service`、`auth.signed_headers` 对不上 |
| 限流（`api.throttled`，5） | `RequestLimitExceeded` |
| 参数被拒（`api.invalid`，6） | `InvalidParameter` / `UnknownParameter`；`X-TC-Region` 这类只能走 header，⛔ 别两处都写 |
| 缺 `auth.service`（`am.invalid`，2） | `--check` 会点名 source 与 `auth.service`，并给 `"service": "<该接口所属服务名>"` 的补法 |

---

## 五 · 第 4 步：读回执（**这一段决定你会不会误导用户**）

普通模式里，**行回执**（`format="rows"`）的 `rows` 之外有四个字段；**图回执（默认）**把它们放在 `meta` 里、用 `summary` 给 `nodes`/`row_count`（`@graph` 里没有 `rows`）——**它们不是装饰**：

| 字段 | 含义 | 你必须做什么 |
| --- | --- | --- |
| `warnings[]` | 覆盖范围警告（如"只覆盖主账号"） | **逐条转达**，不许省略 |
| `coverage[]` | 逐表覆盖等级 | 若不足全量，说明范围 |
| `truncated` | 是否被行数上限截断 | 截断就说"结果不完整" |
| `join_states[]` / `evidence_level` | 关系可信度（实测 / 继承 / 未验证） | 低可信度时说"这条关系未经验证" |
| **`meta.complete`** | 这份结果**是不是完整的** | `false` ⇒ 还有 `@pending` 没取；⛔ **不许当完整答案用** |

> **最危险的失败不是查不到，而是"数字对但范围不对"**。
> 用户拿着漏了子账号的金额去做决策，比报错严重得多。

---

## 六 · 第 5 步：怎么把限制说出口（可直接用）

| 场景 | 话术 |
| --- | --- |
| 有覆盖警告 | 「这个数**只覆盖 <范围>**，<范围外> 的没算进去。」 |
| 结果被截断 | 「这是**前 N 行**，不是全部。」 |
| 关系未验证 | 「这条关联是按命名推断的，**没有实测过**。」 |
| 本体模式 | 「这套部署**只有本体、没有数据**；我能告诉你'有哪些概念、口径怎么算'，给不出具体数值。」 |
| 口径不唯一 | 「这个词在系统里有**多种算法**（A / B），你要哪一个？」 |

---

## 七 · 错误 → 动作

| 错误 | 含义 | 你该做什么 |
| --- | --- | --- |
| `401` | 没带 token / token 不对 | 检查 `Authorization: Bearer`（⛔ 不支持 query string 传 token） |
| `403 TENANT_MISMATCH` | 请求里带了**别的租户** | 删掉租户条件，让服务端注入 |
| `403 TOKEN_KIND_FORBIDDEN` | token 类型不对（如管理口令查数据） | 换该部署给的**查询**口令 |
| `404` / `400` | 类名/属性名写错 | 回 `catalog` / `describe` 核对**准确名字**（大小写敏感） |
| `503 NO_DATA_SOURCE` | **需要库**的查询（库源类 / 混合 / `sample` / 没装映射） | **别重试**；若查的是 **api 类**则不该出现这个错 ⇒ 先看 `access.kind` |
| `SPARQL_UNSUPPORTED` | 用了 v1 不支持的语法 | 按 `hint` 改写成支持集内的查询 |
| `NOT_DB_SOURCE` | 查了 API 源的类（rows 模式） | 改走 `@pending` 的 `get`；或先看 `catalog` 的 `access.kind` |
| `CROSS_SOURCE_UNSUPPORTED` | 查询需要跨源遍历 | **拆成两段**：先查库拿 ID，再按 ID 调接口；⛔ 别重试 |
| `429` | 并发/行数超限 | 降并发或加 `LIMIT` |

---

## 八 · 禁忌

1. ⛔ **跳过取本体直接查** —— 会拿用户的词猜字段，猜错不报错
2. ⛔ **省略 `warnings`** —— 用户会以为数字是全部
3. ⛔ **把 503 当"再试一次"** —— 那不是临时故障
4. ⛔ **自己编口径** —— 口径定义在本体里（`rdfs:comment`），照抄，别发挥
5. ⛔ **在查询里写租户** —— 服务端注入；写了会被拒
6. ⛔ **把 `excluded_relations` 当"不存在"** —— 先看证据等级再下结论
7. ⛔ **猜地址** —— 连到别的环境比连不上更糟
8. ⛔ **把 `complete: false` 当答案** —— 那是半成品；`@pending` 必须处理完
9. ⛔ **拿 SPARQL 反复试 api 源** —— TTL 里 `sourceKind "api"` 已经告诉你平台不执行它
10. ⛔ **以为默认会回表格** —— 默认是 JSON-LD 图；要表必须显式传 `format="rows"`
11. ⛔ **自己再手写一份取数脚本** —— 用 `access_fetch.py`（§4.5）；手写一份就多一份对不齐的实现
12. ⛔ **把 `meta.truncated: true` 当"取全了"** —— 那是没取全；退出码 8 就是在说这件事

---

## 九 · 一条完整路径（中性示例）

```
① GET /api/health                      → mode 有值？还是没有该字段？
② GET /api/ontology.ttl                → 读出类 X、属性 p1/p2、关系 X→Y、口径 m
③ GET /api/catalog?page=1              → 确认 X 与 Y 的确切名字
④ 用户问「X 的情况」
   ├─ 本体模式 → 用 TTL 里的定义回答，并说明没有数值
   ├─ X 是 db 源 → SELECT ?x ?v WHERE { ?x a :X . ?x :p1 ?v } LIMIT 20
   └─ X 是 api 源 → 平台给 @pending，用 `access_fetch.py` 取（§4.5；未签的取不了）
⑤ 读回执（默认是图：看 `summary` + `meta`；要表先传 `format="rows"`）：warnings / coverage / truncated / evidence_level
⑥ 若 `meta.complete: false` ⇒ 先处理完 `@pending` 再答；答案里标出哪部分来自外部接口
⑦ 回答里带上限制；不确定的地方说「不确定」，不要补全
```
