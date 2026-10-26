---
name: ontok-query
description: 用 ontok 本体服务回答业务问题。核心顺序是「先取本体（TTL）→ 定位概念 → 决定能否查数据 → 解读回执 → 转达限制」。含 v1 SPARQL 边界、两种部署模式的区别、以及每种错误的处置。适用于任意接入了 ontok 的环境，不含任何具体业务名词。
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
:cvm_instance a owl:Class ;
    rdfs:label "CVM 实例"@zh ;
    ontok:sourceKind "api" .        # ← 这个类的数据不在这套本体里，在外部接口
```

| 标记 | 含义 | 你该怎么做 |
| --- | --- | --- |
| 有 `ontok:sourceKind "api"` | 平台**不执行**这类数据源 | 查它只会拿到 `@pending`；要**按指引自己调接口** |
| **没有**这个谓词 | 库源（平台能查） | 正常写 SPARQL |

**写查询之前先扫一遍**——不然你会拿 SPARQL 去查 api 类，然后拿到空图还以为是"没数据"。

### 大本体怎么读

TTL 可能只有几十 KB，也可能几百 KB：

| 情况 | 做法 |
| --- | --- |
| 小（几十 KB） | 直接读完 |
| 大（几百 KB） | ⛔ 别整份塞进上下文；改用 `catalog` 分页浏览 + `describe` 看单个概念 |

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
| **API 源类**（`access.kind = "api"`） | **200 + `@graph: []` + `@pending` + `complete: false`** | ✅ **按 `@pending[].get` 自己去取数**（见 §4.3） |
| **混合**（库类 + api 类） | `503` | 它确实需要库 ⇒ 拆开：先问本体、再按 `get` 取 |

**怎么提前知道查的是哪类**：`describe` / `catalog` 的 `access.kind`（`db` 走 SPARQL、`api` 走 fetch）。
或者直接看 TTL 里这个类有没有 `ontok:sourceKind "api"`。

⛔ **不要因为"这套是本体模式"就放弃 `@pending`**——那正是它能给的最有价值的东西。

---

### 4.3 API 源：平台只给「取数指引」，数据你自己取

对 `access.kind = "api"` 的类，平台返回的是**半成品**：

```json
{ "@graph": [],
  "@pending": [
    { "@type": "ontok:cvm_instance", "reason": "source_is_api",
      "get": { "class": "cvm_instance",
               "filter": {},                       // ← 注意这一项，见下
               "pushdown": ["region","zone"],
               "ref": "access#CvmInstance@v1" } } ],
  "meta": { "complete": false, "unresolved_classes": ["cvm_instance"] } }
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
| 直接**无过滤全量拉取** | 先用该类的标识属性（`describe` 看，如 `:cvm_instance__instance_id`）**在你自己的取数侧**拿到标识，再带 `ids` 去取 |
| 把 `pushdown` 当"随便筛" | 暂时拿不到标识时，**至少用 `pushdown` 里的字段收窄**范围 |

> **账单、告警一类接口全量拉 = 烧钱 + 触发限流。** 这不是保守，是会被计费的。

**取到之后**：

1. 用 `@id` / `@type` 与平台给的图**合并**（`@context` 里有 `ontok` 前缀）
2. 答案里**标出**哪部分来自外部接口
3. `meta.complete` 现在是 `false` —— 处理完 `@pending` 再作答

**跨源边**：如果两个类一个在库、一个在 API，那条边**不会给你 `get`** ⇒ 拆两段：先查库拿 ID，再按 ID 调接口。

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
8. ⛔ **以为默认会回表格** —— 默认是 JSON-LD 图；要表必须显式传 `format="rows"`

---

## 九 · 一条完整路径（中性示例）

```
① GET /api/health                      → mode 有值？还是没有该字段？
② GET /api/ontology.ttl                → 读出类 X、属性 p1/p2、关系 X→Y、口径 m
③ GET /api/catalog?page=1              → 确认 X 与 Y 的确切名字
④ 用户问「X 的情况」
   ├─ 本体模式 → 用 TTL 里的定义回答，并说明没有数值
   ├─ X 是 db 源 → SELECT ?x ?v WHERE { ?x a :X . ?x :p1 ?v } LIMIT 20
   └─ X 是 api 源 → 平台给 @pending，按 get 调接口自己取
⑤ 读回执（默认是图：看 `summary` + `meta`；要表先传 `format="rows"`）：warnings / coverage / truncated / evidence_level
⑥ 若 `meta.complete: false` ⇒ 先处理完 `@pending` 再答；答案里标出哪部分来自外部接口
⑦ 回答里带上限制；不确定的地方说「不确定」，不要补全
```
