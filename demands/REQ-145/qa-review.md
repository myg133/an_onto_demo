# QA 审核：REQ-145 · 平台侧支持 API 源（`sourceKind` + `@pending` + `--access-mapping`）

> Pre-merge 审核。审核人：QA Agent。审核时间：2026-10-25。
> 判据来源：`BA/demands/REQ-145/demand.md` + `acceptance.md`（9 条 AC + **§三 裁定 R-A~R-D**）+
> `BA/design/2026-10-25-interaction-contract.md`（§3.2 已按 R-A 降级为「阶段 2 目标形态」）。
> **一律以自己实测为准**；Dev 自报的数字与 `.local/ac145/` 下的旧产物只当线索（见 N-1）。

---

## 0 · 审核对象与环境

| 项 | 值 |
| --- | --- |
| 被测 | `feature-REQ-145` @ **`a60b9d8`**（`feature/REQ-145`，2 提交：`d32738f` + `a60b9d8`），worktree 全程只读、`git status` 干净 |
| 基线 | `develop` **`b4a8b56`**（worktree `code/`，同为 release 构建） |
| 改动面 | **23 个文件 `+2700 / −23`**：新增 `src/query/access.rs`(977) + `tests/test_req145_live.rs`(719)；改 `mod.rs`/`jsonld.rs`/`catalog.rs`/`audit.rs`/`error.rs`/`http.rs`/`serve.rs`/`ttl.rs`/`cli.rs`/`domain/ttl.rs`/`console/ops.rs`/`portal/serve.rs`/`server/http.rs` + 8 个既有测试文件（只补偿新参数 `api_classes: None` / `access: None` / `pending: false`） |
| 自建二进制（基线） | `.local/cargo-qa145-base` ⇒ `ontok.exe` **16 034 304 B** · sha256 `4DBC78C3F897903C4C2F0109D9A34750A8C399981474C6F17A45AACB8826C5E3` |
| 自建二进制（特性） | `.local/cargo-qa145-feat` ⇒ `ontok.exe` **16 179 712 B** · sha256 `15306A8D8F0F8F7ABF9CA582DD59B7347A8CC8E4C21353ED7CF626EDE5FC4638` |
| 工具链 | cargo 1.96.0 / rustc 1.96.0（两边**同 profile** release，`cargo build --release --bin ontok`） |
| 元数据库 | `postgresql://postgres:postgres@172.17.0.87:5432/anspire_real`（只读：`query serve` 的只读会话） |
| 源库 | `postgresql://postgres:postgres123@172.16.120.20:30432/anspire_open_uat`（只读） |
| 项目 | `anspire_r2rml_excl` v2（catalog 实测 **84 类**） |
| 映射 | `.local/ac145/access.json`（`mapping_version=2`，1 个 api 类 `customers`，`pushdown:["ids"]`，`freshness:realtime`） |
| 服务实例 | 5 个：`:8911` 基线无映射 · `:8912` 特性无映射 · `:8913` 特性有映射（+`--audit-out`）· `:8914` 基线 +audit · `:8915` 特性有映射 +audit（**全部已停**） |
| 临时目录 | `.local/qa145/`（证据，**保留**）· `.local/cargo-qa145-{base,feat,mut}`（build，**已清理**） |

**自建理由**：Dev 的 `.local/ac145/*.exe` 我不采信（同一目录里还留着**改前**的产物，见 N-1）。
两边二进制都由我从**各自 worktree 的源码**独立构建，产物只经 HTTP 出口取，不经 Dev 脚本。

---

## 1 · 结论

> ## **PASS（带非阻断）**
>
> 9 条 AC **全部实证通过**；R-A / R-B / R-C / R-D 四条裁定**逐条落地且我有独立判据**；
> 三条变异（`sourceKind` 写给库源 / `complete` 恒 true / rows 返回空表）**3/3 全红且红在预期断言上**；
> 零回归 **16/16 逐字节相同**；不退化（四红线 + `tools.rs` + `property_filter.rs` + `Cargo.*` 零 diff、
> 出口 63 条不变、零新依赖、测试 9 + 756 全绿）。

| # | 审核项（= P0 清单） | 判 | 一眼结论 |
| --- | --- | --- | --- |
| 1 | R1 零回归（16 项产物逐字节） | ✅ | **16/16 相同**，sha 清单见 §2 |
| 2 | TTL `sourceKind`（仅 api、两视图、0 泄露）+ 变异 | ✅ | 两视图各 **+1 行**；`sourceKind` 计数 **1/0**；11 个禁用词 **0 命中**；**M1 红** |
| 3 | `describe`/`catalog` 的 `access` | ✅ | `customers`⇒`api/fetch`，`order`⇒`db/sparql`；84 个类各一条 |
| 4 | `@pending` + `complete`（四组 + 变异） | ✅ | 四组不变式全成立；**M2 红** |
| 5 | rows 模式 400 且无 `rows` 字段 | ✅ | 400 `NOT_DB_SOURCE`；`"rows"`/`row_count` **0 处**；**M3 红** |
| 6 | 跨源边（no `get` / `ref`+`local_hint` / 反向谓词 / 400） | ✅ | 三处全对，**反向谓词我自己验了** |
| 7 | 审计与响应一致（R-B） | ✅ | `OK` / `NOT_DB_SOURCE` / `OK` 三行全带 `pending=true`；`API_PENDING` **0 命中**；非 pending 行**字节不变** |
| 8 | R-3 文案 + `@graph` 恒空 | ✅ | `--help` 有「整体 pending / 一行 SQL 都不执行 / `@graph` 恒空」；`@graph` 由构造保证空 |
| 9 | 不退化 / 零依赖 / 出口 / 测试数字 | ✅ | 零 diff 清单见 §10；**756/0**（基线 745/0，+11） |

**非阻断（5 条，均不影响合并，见 §12）**：
N-1 Dev 的 `.local/ac145/mapped/` 产物是**改前**的（无 `ref`/`local_hint`）·
N-2 启动横幅「跨源边 80 条」把前向+反向各算一次（真实 40 条）·
N-3 核心 16 项里有 3 项在有映射时**收敛成同一份字节**（判据覆盖面说明）·
N-4 R-D「点名 api 属性 ⇒ 400」的字面读法与实现不一致（AC 文本该收紧，代码没错）·
N-5 客户视图按 **`display_name`** 匹配 api 类（同名合并是它的结构特性）——`display_name` 为空的 api 类会在客户视图漏标（本数据不失灵）。

---

## 2 · 零回归 16/16（最硬的一条）

**做法**：基线 `:8911` 与特性 `:8912` **都不给 `--access-mapping`**，同一份请求脚本打 16 项，
落盘为字节后比 sha256（`SPARQL` 都是 `POST /api/sparql`，`format` 显式）。

| # | 产物 | 请求 | sha256（**两边完全相同**） |
| --- | --- | --- | --- |
| 1 | `ttl-customer.ttl` | `GET /api/ontology.ttl` | `a420bbcefb040746…c899cc070` |
| 2 | `ttl-model.ttl` | `?view=model` | `21e134ef1fc4460e…232383e9a8dd84b` |
| 3 | `catalog.json` | `/api/catalog?page=1&page_size=200` | `f2cd2fb8e847a004…afc79bee4bf739` |
| 4 | `describe-customers.json` | `/api/describe?name=:customers` | `eddc488375cccb0c…5760dfb5de2245c0` |
| 5 | `describe-order.json` | `/api/describe?name=:order` | `482a67996a62b328…1916b6b8d4381da` |
| 6 | `sparql-rows.json` | rows · `?c a :customers . ?c :customers__id ?id` | `50ca0ac5a99eb0a9…2150884918e12ff1` |
| 7 | `sparql-jsonld.json` | jsonld · 同上 | `d7b9e4d981a61272…768059ccac4a20` |
| 8 | `sparql-agg-rows.json` | rows · `COUNT(?c)` | `6e1a81202985a97c…6702b63728d243` |
| 9 | `sparql-order-rows.json` | rows · `order__amount` | `8ee2db9f52280732…864c212411779700` |
| 10 | `api-class-jsonld.json` | jsonld · `?c a :customers` | `d117fa4c2c04d764…ce216e7372` |
| 11 | `api-class-jsonld-id.json` | jsonld · 带 `__id "7d49…"` | `712da9d8099c61f6…78e3cae5d` |
| 12 | `api-class-rows.json` | rows · `?c a :customers` | `92b018386f83584a…3f235fa` |
| 13 | `cross-edge-jsonld.json` | jsonld · `?o :order__customers ?c` | `e5d9937b613cb93d…5b94b8b6` |
| 14 | `cross-walk-jsonld.json` | jsonld · 跨源边 + api 属性 | `22b6811170507b98…7394526f` |
| 15 | `cross-walk-rows.json` | rows · 同上 | `1a7fde0207be033d…3a92c26bdf` |
| 16 | `mixed-jsonld.json` | jsonld · `?o a :order . ?c a :customers` | `72b1f31710cb8d80…6f4223dd` |

实测比对输出：

```
SAME=16 DIFF=0  total=16
--- STATUS diff:      （Compare-Object 无输出 = 连状态码序列也相同）
基线 17 个文件 / 特性 17 个文件（16 产物 + STATUS.txt）
```

**附带自查**（不回退、也不偷偷变）：

| 检查 | 结果 |
| --- | --- |
| 无映射时 `mapping_version` / `@pending` / `access` 出现 | **0 / 0 / 0** 次（`catalog.json`、`api-class-jsonld.json`、`sparql-jsonld.json`） |
| 无映射时 TTL 两视图字节 | 293 814 / 367 653 B —— 与基线**逐字节相同**（即 §2 第 1、2 行） |
| `--access-mapping` 参数本身 | 只在给了之后生效；不给 ⇒ 运行期一切照旧（`access = None`） |

---

## 3 · TTL `sourceKind`（AC-001）+ 变异

**两视图各只加一行**（映射 vs 不映射，`Compare-Object` 的差异行就是全部差异）：

```
ttl-customer.ttl   nomap 8080 行 → mapped 8081 行（+1）
  => ontok:sourceKind "api" .
  => ontok:instanceIri "https://ontok.local/anspire_r2rml_excl#{class}/{id}" ;   ← 同一行末由 . 改 ;
  <= ontok:instanceIri "https://ontok.local/anspire_r2rml_excl#{class}/{id}" .

ttl-model.ttl      nomap 10084 行 → mapped 10085 行（+1）
  => ontok:sourceKind "api" ;
```

落点核对（**加在类块上，且只有 api 类**）：

```turtle
# 客户视图（按人读名匹配）
:客户 a owl:Class ;
    rdfs:label "客户"@zh ;
    ontok:instanceIri "https://ontok.local/anspire_r2rml_excl#{class}/{id}" ;
    ontok:sourceKind "api" .

# 建模视图（按类 key 匹配）
:customers a owl:Class ;
    rdfs:label "客户"@zh ;
    ontok:key "customers" ;
    ontok:instanceIri "https://ontok.local/anspire_r2rml_excl#{class}/{id}" ;
    ontok:sourceKind "api" ;
    ontok:primaryKeyColumn "id" .
```

| 判据 | 实测 |
| --- | --- |
| 只 api 类有 | 两视图 `sourceKind` 计数：**mapped = 1 / nomap = 0**；`order`/`balances`/其余 83 类**没有** |
| 两视图都有 | ✅ 上面两段 |
| 零泄露（11 个词 × 两视图） | `tencent` / `DescribeCustomers` / `CustomerSet` / `pushdown` / `iterator` / `max_scan_rows` / `access#` / `@v2` / `realtime` / `reviewed_by` / `7d491821` ⇒ **全部 0 命中** |
| **变异 M1**：库源也写 `sourceKind` | `src/domain/ttl.rs` 两处 `if is_api {` → `if true {` ⇒ **`ac001` 当场红**：`tests\test_req145_live.rs:302` `客户视图：只有 api 类写 sourceKind`，输出里 84 个类全被标（红样本见 §11） |

> 结构上的第二道保险：TTL 只拿到 `ApiClasses`（`is_api_key` / `is_api_name` 两个布尔查询），
> 端点/参数/凭据**没进这个结构**，所以「漏出去」不是靠人记得别写。

---

## 4 · `describe` / `catalog` 的 `access`（AC-003）

各一条原文（`GET /api/describe?name=…`）：

```json
// :customers（api 类）
"access": {"kind":"api","queryable_by":["fetch"],"pushdown_fields":["ids"],"freshness":"realtime"}

// :order（库源类）
"access": {"kind":"db","queryable_by":["sparql"]}
```

`catalog` 侧：

```
catalog class items = 84；带 access 的 = 84；其中 kind=api 的 = 1（customers）
样例：activity  {"kind":"db","queryable_by":["sparql"]}
      customers {"kind":"api","queryable_by":["fetch"],"pushdown_fields":["ids"],"freshness":"realtime"}
```

- `pushdown_fields` / `freshness` **只在映射写了时出现**（映射里没写 ⇒ 键不出现，不编）。
- 无映射时 `access` 键**完全不出现**（§2 已由逐字节相同兜住）。
- 顺带验了「api 类即使没有 TriplesMap 也要能 describe」这条路：`customers` 有映射，走的是普通路，未触发兜底分支（属未覆盖路径，不阻断）。

---

## 5 · `@pending` + `complete`（AC-004 / AC-007）

### 5.1 · 打 api 类（jsonld）——原文

`POST /api/sparql?format=jsonld` · `SELECT ?c ?n WHERE { ?c a :customers . ?c :customers__customer_name ?n }` → **200**

```json
{"@context":{…},"@graph":[],
 "@pending":[{"@type":"ontok:customers","ontok:sourceKind":"api","reason":"source_is_api",
   "get":{"class":"customers","filter":{},"pushdown":["ids"],"ref":"access#Customer@v2"}}],
 "summary":{"nodes":0,"pending":1,"row_count":0},
 "meta":{…,"complete":false,"unresolved_classes":["customers"],"mapping_version":2}}
```

查询里**认得出实例 id** 时（AC-004 的 `filter.ids`）：

```json
"@pending":[{"@id":"ontok:customers/7d491821-27c3-4bf8-8c31-19ee5acd936d","@type":"ontok:customers",
  "ontok:sourceKind":"api","reason":"source_is_api",
  "get":{"class":"customers","filter":{"ids":["7d491821-27c3-4bf8-8c31-19ee5acd936d"]},
         "pushdown":["ids"],"ref":"access#Customer@v2"}}],
"meta":{…,"complete":false,"unresolved_classes":["customers"],"mapping_version":2}
```

| AC-004 判据 | 实测 |
| --- | --- |
| `@graph: []` | ✅ `[]` |
| `@pending` 一条 | ✅ 1 条 |
| `meta.complete: false` | ✅ |
| `unresolved_classes` 含该类 | ✅ `["customers"]` |
| `get` 含 `class`/`filter`/`pushdown`/`ref` | ✅ 四个键齐；`ref` = `access#Customer@v2`（带 `@v2`） |
| `@pending` **不进** `@graph` | ✅ 结构上是两个字段；`@graph` 里 0 节点 |

### 5.2 · 不变式 `complete == @pending.is_empty()`（四组）

| # | 组 | 查询 | `@pending` | `meta.complete` | 不变式 |
| --- | --- | --- | --- | --- | --- |
| ① | api 类 | `?c a :customers . ?c :customers__customer_name ?n` | 1 条（`source_is_api`） | `false` | ✅ `false == false` |
| ② | 库源**有行** | `?b a :balances` LIMIT 3 | `[]`（1 节点） | `true` | ✅ `true == true` |
| ③ | 库源**0 行** | `?o a :order . ?o :order__id "__no_such_id__"` | `[]`（0 节点，SQL 真跑过） | `true` | ✅ `true == true` |
| ④ | **跨源边** | `?o a :order . ?o :order__customers ?c` | 1 条（`cross_source_edge`） | `false` | ✅ `false == false` |

②的原文（说明「完整」不是靠空图混过去的）：`"@graph":[{"@id":"ontok:balances/e75615b9-…","@type":"ontok:balances"}]`,
`"row_count":1`, `"complete":true`, `"unresolved_classes":[]`；
③的原文：`row_count:0` 且 `sql_echo` 里有真 SQL（`… FROM "order" t0 WHERE … AND t0."id" = '__no_such_id__'`）, `complete:true`
⇒ **「0 行」与「少了一块」在字段上可分**，这是本需求最核心的语义。

- `unresolved_classes` 与 `@pending` **同源**：四组里 `unresolved.is_empty() == pending.is_empty()` 全成立。
- **变异 M2**：`jsonld.rs` 的 `let complete = nodes.is_empty();` → `let complete = true;` ⇒ **`ac007` 红**：
  `不变式 complete == @pending.is_empty() 被破坏（complete=true, pending_empty=false）`（§11）。

---

## 6 · rows 模式（AC-005）

`POST /api/sparql?format=rows` · `SELECT ?c WHERE { ?c a :customers }` → **400**

```json
{"code":"NOT_DB_SOURCE",
 "hint":"① 改要 **JSON-LD**：`?format=jsonld` —— 会拿到 `@graph: []` + `@pending[]`（每条带 `get.class` / `get.filter` / `get.pushdown` / `get.ref`），\n  ② 再按 `get` 去取（凭据由你管）：`filter.ids` 里就是平台从查询里认出来的实例标识，\n  ③ 平台**不做**跨源 join：…拆成两段查…，\n  ④ 想知道哪些类要这么走：`GET /api/catalog` / `/api/describe` 看 `access.kind`…\n  ⛔ 重试无效：…",
 "message":"查询落在 **API 源**上（customers）：这些类的数据不在本库，平台**不执行** API 调用，行回执（`format=rows`）无法表达「少了一块」。"}
```

| 判据 | 实测 |
| --- | --- |
| 状态码 | **400** |
| 正文里 `"rows"` 键 | **0 处** |
| 正文里 `row_count` | **0 处** |
| 混合查询用 rows（库源+api） | 同样 400 `NOT_DB_SOURCE`（不是「只返回库源那半」） |
| 同一条查询用 jsonld | 200 + `@pending`（§5.1） |
| `sample` 打 api 类（R-D 第二半，MCP `tools/call`） | `isError:true` + `structuredContent.code = NOT_DB_SOURCE` + 同一条 hint |
| **变异 M3**：rows 返回空表 | `run_sparql` 的 Pending 分支改 `Ok(p.receipt)` ⇒ **`ac005` 红**，红样本正文里 `"rows":[]`、`"row_count":0` 出现了（§11） |

---

## 7 · 跨源边（R-A 的核心）

### 7.1 · 只写关系谓词 ⇒ pending **没有 `get`**，但有 `ref` + `local_hint` + 三步 `hint`

`SELECT ?o ?c WHERE { ?o a :order . ?o :order__customers ?c }` → 200 原文：

```json
"@pending":[{"@type":"ontok:customers","ontok:sourceKind":"api","reason":"cross_source_edge",
  "ref":"access#Customer@v2",
  "local_hint":{"uses_column":"customer_id",
                "get_ids_by":"SELECT DISTINCT \"customer_id\" FROM \"order\""},
  "hint":"① 先在库侧取 ID：`SELECT DISTINCT \"customer_id\" FROM \"order\"`（按你的租户过滤条件收窄）；
          ② 再按这些 ID 调 access#Customer@v2 指向的映射取数据；
          ③ 把两段结果拼起来 —— 平台不做跨源 join。"}]
```

| R-A 判据 | 实测 |
| --- | --- |
| **没有 `get`** | ✅ 该条 pending 里 `get` 键不存在（`get` 只出现在按类点名那一条） |
| **有 `ref`** | ✅ `access#Customer@v2` |
| **有 `local_hint.uses_column`（库侧 FK 列）** | ✅ `customer_id` |
| 附带可执行取数语句 | ✅ `SELECT DISTINCT "customer_id" FROM "order"`（不含租户条件，hint 里明说要自己收窄） |
| **三步 `hint`** | ✅ ①②③ 三步齐全 |
| `reason` 是稳定字符串 | ✅ `cross_source_edge`（不是人话） |

### 7.2 · 反向谓词 `:customers__order` —— **我自己验的**（Dev 只在单测里覆盖）

| 查询 | 结果 | 判 |
| --- | --- | --- |
| `SELECT ?c ?o WHERE { ?c :customers__order ?o }`（**只写反向谓词**） | 200，`reason=cross_source_edge`，**无 `get`**，`ref` + `local_hint.uses_column=customer_id` + 三步 `hint` | ✅ 与 `order__customers` **字节完全相同**（sha `f0514142…`） |
| `SELECT ?c ?n WHERE { ?c :customers__order ?o . ?c :customers__customer_name ?n }`（反向 + 点名 api 属性） | **400** `CROSS_SOURCE_UNSUPPORTED`（提示「边 `order__customers` 一端在库源（`order`）、另一端在 API 源（`customers`）」） | ✅ 两种格式都是 400 |

⇒ 「只登记前向」会漏判的坑**没有踩**：`bind()` 把同一个 `CrossEdge` 按前向名与反向名**各登记一次**，
反向谓词写出来的遍历**没被当成库源查询去跑 SQL**（这是最容易静默错的一种）。

### 7.3 · 点名 api 属性（跨源遍历）⇒ 400 `CROSS_SOURCE_UNSUPPORTED`

`?o a :order . ?o :order__customers ?c . ?c :customers__customer_name ?n`：

| format | 状态 | code | hint 是否可行动 |
| --- | --- | --- | --- |
| jsonld | **400** | `CROSS_SOURCE_UNSUPPORTED` | ✅ 拆两段 + 「⛔ 别重试同一条查询」+ 指向 `catalog` 看 `access.kind` |
| rows | **400** | 同上（**同一份正文**，sha 相同） | ✅ |

**⚠️ 口径说明（N-4）**：如果查询**只**点名 api 属性、**没有**写跨源边
（`SELECT ?n WHERE { ?c :customers__customer_name ?n }`），实现给的是 **200 + `@pending`（带 `get`）**，
不是 400。我判**实现是对的**：这种查询「只涉及 api 类」，正是 AC-004 的形状；
400 只应发生在「跨源边**还要往下走**」时。建议 BA 把 R-D 的措辞从「点名 api 属性 ⇒ 400」
收紧成「**跨源遍历中**点名 api 属性 ⇒ 400」——这是 AC 文本的精度问题，不是代码缺陷。

---

## 8 · 审计与响应一致（R-B）——最容易漏的一条

**做法**：`:8915` 起服务时带 `--audit-out`，**新文件**，按派单顺序打三次
（jsonld api 类 / rows api 类 / jsonld api 类），再读审计原文；另在 `:8914`（**基线二进制**）
打**同一条库源查询**做「既有行形状」对拍。

三次调用的审计原文（**一字未改**）：

```
audit ts=…08:50:02.696Z event=sparql proto=rest token=sha256:0dad47b2664a tenant=7d49…936d q=sha256:fa8e34c4e95acaaa rows=0 elapsed_ms=0 code=OK pending=true
audit ts=…08:50:03.549Z event=sparql proto=rest token=sha256:0dad47b2664a tenant=7d49…936d q=sha256:7b0d680c661b51c2 rows=0 elapsed_ms=0 code=NOT_DB_SOURCE pending=true
audit ts=…08:50:04.458Z event=sparql proto=rest token=sha256:0dad47b2664a tenant=7d49…936d q=sha256:fa8e34c4e95acaaa rows=0 elapsed_ms=0 code=OK pending=true
```

| 判据 | 实测 |
| --- | --- |
| 三次的 `code` | **`OK` / `NOT_DB_SOURCE` / `OK`**（= 客户端实际收到的 200/400/200） |
| 三次都有 `pending=true` | ✅ |
| `pending` 是**独立字段**，不改 `code` | ✅ 位置固定在 `code=` 之后；`rows=-`/`rows=0` 语义不变 |
| 全文 grep `API_PENDING` | **0 命中**（`:8913`/`:8914`/`:8915` 三个审计文件全查） |
| `pending=false` 的既有行**字节不变** | ✅ 基线首行 vs 特性（**有映射**）同一条库源查询，归一化 `ts=`/`elapsed_ms=` 后 **183 B 对 183 B、`-ceq` 为 True**；两边都**不含** `pending=` |
| 一次调用一行审计 | ✅ 24 次调用 = 24 行（`:8913` 文件），pending 那条没有重复记两行 |

> 附加事实：`CROSS_SOURCE_UNSUPPORTED` 的审计行仍是既有错误形状（`rows=-`、**无** `pending=true`）——
> 这是对的：**硬错误不是「半个答案」**，不该被标 pending。

---

## 9 · R-3 文案与 `@graph` 恒空

`ontok query serve --help`（实跑，非读源码）里：

```
⚠️ **已知限制（R-3）**：一条查询**同时**涉及库源实体与 API 类时，整个查询
返回 pending（`meta.complete:false`）、**一行 SQL 都不执行**、`@graph` 恒空 ——
⛔ 不会塞半份库源结果伪装成「算完了」。部分求值（库源那半照算）要拆查询计划，
**是阶段 2 的事**；本期请拆成两条查询（先库源，再按 `get` / `ref` 取 API 那半）。
```

关键词命中：`已知限制（R-3）` **1** · `一行 SQL 都不执行` **1** · `@graph` **2** · `恒空` **1** · `阶段 2` **1** · `--access-mapping` **2**。
（「混合查询」这个词本身没出现，但限制说的是同一件事——一条查询同时涉及库源实体与 API 类。）

**`@graph` 恒空是构造保证**：混合查询（`?o a :order . ?c a :customers`）实测
`"@graph":[]`、`"sql_echo":""`、`"tables":[]`、`pending:1`、`complete:false`，
审计那行 `rows=0`。代码上 `render_pending()` 里 `graph: Vec::new()` 是**唯一**赋值点，
`render_with_mapping()` 的 `pending` 只从 `receipt.complete` 派生（库源路径恒空）⇒ 不存在「半份结果进图」的路。

---

## 10 · 不退化（AC-009）

| 项 | 判据（`git diff --numstat b4a8b56..HEAD -- <path>`） | 结果 |
| --- | --- | --- |
| 禁改面（四红线 + `tools.rs`(=`guard()`) + `property_filter.rs`）**实测零 diff 的路径** | `src/mcp/tools.rs` · `src/domain/property_filter.rs` · `src/domain/view.rs` · `src/domain/r2rml.rs` · `src/domain/pipeline.rs` | **全部无输出（零 diff）**；`src/mcp/tools.rs` 与基线**逐字节相同** ⇒ `guard()` 一字未动 |
| 新增文件只有两个 | `git diff --name-status` | `A src/query/access.rs`（纯判定/取值，**不写出**）· `A tests/test_req145_live.rs`；⛔ 没有任何新出口文件 |
| `Cargo.toml` / `Cargo.lock` | 同上 | **零 diff ⇒ 零新依赖** |
| `#[ignore]` / `#[should_panic]` 新增 | 扫 `tests/` + `src/` 的 `+` 行 | **0 处**（也没有删测试：`git diff --diff-filter=D` 无删除） |
| 出口 | `OUTLETS` 条目数（`src/domain/pipeline.rs` 的 `Outlet {` 行） | 基线 **63** / 特性 **63** ⇒ **未新增写点**；`access` 模块在 `tests/test_req107_source_enumeration.rs` 的 T6 表里登记为**非出口模块**（登记理由：只做判定，产物经既有出口 I14/I25 等送出） |
| 出口登记守卫 | `cargo test --test test_req107_source_enumeration` | **10 passed / 0 failed** |
| TTL 字节判据 | `cargo test --test test_req127_live` | **2 passed / 0 failed** |
| REQ-145 现场判据 | `cargo test --test test_req145_live`（`ONTOK_REQUIRE_FIXTURES=1`） | **9 passed / 0 failed**（10.86 s） |
| lib 数字（**同机自测**） | 特性 `cargo test --release --lib` | **756 passed / 0 failed** |
| lib 基线 | 基线 `cargo test --release --lib` | **745 passed / 0 failed** ⇒ **Δ +11**（= `src/` 里新增的 **11 条** `#[test]`：`query::access` 10 条 + `query::audit` 1 条，逐一对上；9 条 live 属 `--test`、不在这 11 条里） |

> ⚠️ 一个**环境口径**要先说清：`--lib` 若不设 `ONTOK_TEST_DATABASE_URL`，两边都会各有**同 2 条**
> 环境性失败（`db::apply::…unique_conflict…` / `extract::introspect::read_only_pool…`，
> 报「缺 ONTOK_TEST_DATABASE_URL」）。**基线同样红 2 条** ⇒ 与本需求无关；补上该变量后两边 **0 failed**。
> 我报的 756/745 都是**补齐变量后**的数字。

---

## 11 · 变异（纪律 G：三条，全部红）

在 `git archive HEAD` 解出来的**副本**上做（⛔ 未碰 worktree），`CARGO_TARGET_DIR=.local/cargo-qa145-mut`，
每个变异只改一处、跑完反向还原（`MUTANT-M*` 记号归零）。

| 变异 | 改法 | 结果 |
| --- | --- | --- |
| **M1** 库源也写 `sourceKind` | `src/domain/ttl.rs` 两处 `if is_api {` → `if true {` | **红** `ac001` @ `test_req145_live.rs:302`：`客户视图：只有 api 类写 sourceKind（库源默认 db，不写）`，输出 84 个类全被标 |
| **M2** `complete` 恒 true | `src/query/jsonld.rs:402` `let complete = nodes.is_empty();` → `= true;` | **红** `ac007` @ `:605`：`不变式 complete == @pending.is_empty() 被破坏（complete=true, pending_empty=false）` |
| **M3** rows 返回空表 | `src/query/mod.rs:1042-1048` Pending 分支 `Err(e)` → `Ok(p.receipt)` | **红** `ac005` @ `:503`：正文变成 `{"columns":[],…,"row_count":0,"rows":[]…}` |

三个变异都**红在预期的那条断言上**（不是编译错、不是连带别的用例）。还原后同目录重跑 ⇒ `9 passed / 0 failed`。
证据文件：`.local/qa145/mutations.txt`。

---

## 12 · 非阻断（5 条，摊开给 BA 判）

| # | 事项 | 事实 | 我的判断 |
| --- | --- | --- | --- |
| **N-1** | Dev 的 `.local/ac145/mapped/` 产物是**改前**的 | 该目录 `cross-edge-jsonld.json` 879 B、`@pending` 里**没有** `ref`/`local_hint`/`hint`（时间戳 15:47，早于 `a60b9d8`）；我重抓的是 1268 B（含三样） | 不影响交付（`.local/` 未跟踪、非交付物），但**BA 若照它判 R-A 会误判**。建议 Dev 重抓或删掉，避免下一个人当证据 |
| **N-2** | 启动横幅「跨源边 **80** 条」是**双算** | 真实跨源边 **40** 条（TTL 里 `*__customers` 去重 40 个），`cross_edges` 映射为前向+反向各登记一次 ⇒ `len()=80` | 纯文案/口径问题（判定逻辑正确：前向反向都登记**必须**如此）。建议改成「跨源边 40 条（前向+反向谓词 80 条）」或除以 2 |
| **N-3** | 核心 16 项里有 3 项在**有映射**时收敛成同一份字节 | `sparql-jsonld` / `api-class-jsonld` / `mixed-jsonld` 的 sha 都是 `d8a76d47…`（都落到同一条 `source_is_api` pending） | 不影响「16/16 逐字节相同」（那是对**无映射**两边的比对）。但**有映射时的库源 JSON-LD** 该另抓：我补了 `db-order`（0 行，complete=true）/`db-empty`（真跑 SQL，complete=true）/`db-balances`（1 节点，complete=true）三条，判据见 §5.2 |
| **N-4** | R-D「点名 api 属性 ⇒ 400」字面读法与实现不一致 | 只点名 api 属性（无跨源边）⇒ **200 + pending + get**；跨源遍历中点名 ⇒ **400** | **实现对、AC 措辞该收紧**（见 §7.3）。这是 AC 文本问题（纪律 S），不阻断 |
| **N-5** | 两视图匹配键**不同**，其中客户视图按 `display_name` | 建模视图按 `entities[].class`（key）匹配；客户视图按人读名匹配（同名合一是它的结构特性）。⇒ `display_name` 为空的 api 类**在客户视图会漏标**（本数据 `customers` 的 `display_name = "客户"`，实测两视图都标上了）；反过来两个类同名且只有一个在 api ⇒ 客户视图会把两者都标上 | 实测**当前数据不受影响**，属边界风险。若要封，需在 `bind()` 里对 `display_name` 为空/重名的 api 类**拒绝启动或告警**（现在是静默按「能匹配上多少算多少」）；建议登记为已知限制，不阻断 |

---

## 13 · 额外抽查（派单没强制，但会静默出事的地方）

| 抽查 | 做法 | 结果 |
| --- | --- | --- |
| 映射**坏**时是否静默退回「全是库源」 | 类名写错 `customerz` | **exit 1** + `与本体 v2 对不上…最接近的几个：customers` + 「⛔ 拒绝整份映射，而不是忽略那一行」 |
| 缺 `mapping_version` | 删掉该键 | **exit 1** + 「它是 `meta.mapping_version` 与 `get.ref` 的 `@v<n>`…⛔ 不替你编一个」 |
| 映射文件不存在 | 指到不存在的路径 | **exit 1** + 「⛔ 读不出来就当作没给会静默退回『全是库源』」 |
| 启动横幅 | `--access-mapping` 生效时 | `access 映射：mapping_version=2 · api 类 1 个（customers）· 跨源边 80 条`（N-2 的口径问题） |
| MCP 与 REST 是否同一判定 | `tools/call sparql_query`（api 类，jsonld） | 200，`@graph:[]` + `@pending`（`get.ref`）+ `meta.complete:false` ⇒ **同一判定、同一渲染** |
| `sample` 打 api 类（R-D 第二半） | `tools/call sample` | `NOT_DB_SOURCE` + 同一条 hint（与 rows 同口径） |

---

## 14 · 临时目录与清理

| 路径 | 用途 | 现状 |
| --- | --- | --- |
| `.local/qa145/` | **证据**：`out-base-nomap/`、`out-feat-nomap/`（各 16 产物 + STATUS + SHA256SUMS）、`out-mapped/`（16 + 8 条探针）、`audit/`（3 份审计原文）、`help-serve.txt`、`mutations.txt`、`bin/`（两个二进制）、5 个服务的 stderr/stdout、3 个脚本 | **保留**（32.9 MB） |
| `.local/cargo-qa145-base` | 基线 release 构建（887 MB） | **已清理**（二进制已另存 `bin/ontok-baseline-b4a8b56.exe`，sha 见 §0） |
| `.local/cargo-qa145-feat` | 特性 release 构建（982 MB） | **已清理**（二进制已另存 `bin/ontok-req145-a60b9d8.exe`，sha 见 §0） |
| `.local/cargo-qa145-mut` | 变异构建（946 MB） | **已清理**（结果见 §11 与 `mutations.txt`） |
| `.local/qa145/mut/` | `git archive HEAD` 的变异副本树 | **已清理** |
| 5 个服务进程（8911–8915） | `query serve` 实例 | **已全部 Stop-Process**，端口无监听 |
| `D:\` 根 | — | **未新建任何目录**（构建目录全在仓库 `.local/` 下） |
| feature 分支 / worktree | — | **全程未改**：`git status` 干净、HEAD 仍 `a60b9d8`；**未 push** |

> 复现方式：`pwsh -File .local/qa145/start.ps1 -Exe .local/qa145/bin/ontok-req145-a60b9d8.exe -Port 8913 -Tag feat-mapped -Access <access.json> -AuditOut <log> -WorkRoot <dir>`，
> 再 `pwsh -File .local/qa145/capture.ps1 -Port 8913 -Out <dir> -Set all`。
> （重建二进制约 4 min/个，需各自独立 `CARGO_TARGET_DIR`；我这次用的是 `.local/cargo-qa145-base` / `-feat` / `-mut`。）

---

## 15 · 一句话给 BA

**可以合并。** 9 条 AC 全过、零回归 16/16、跨源边「no `get` 但带 `ref`+`local_hint`+三步 hint」与反向谓词都成立、
审计码与客户端响应一致且 `API_PENDING` 归零、`complete` 不变式四组数据全成立、三条变异全红。
需要 BA 动手的只有两件**文本/家务**：把 R-D 的「点名 api 属性」收紧成「跨源遍历中」（N-4），
以及让 Dev 把 `.local/ac145/mapped/` 那批**改前**产物重抓或删掉（N-1）。
