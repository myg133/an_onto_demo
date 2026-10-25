# 需求：REQ-145 · 平台侧支持 API 源（`sourceKind` + `@pending` + 加载映射）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-145 |
| 优先级 | **P0**（两平台交互契约的平台侧） |
| 状态 | 进行中 |
| 基线 | `develop` `b4a8b56` |
| 依据 | `BA/design/2026-10-25-interaction-contract.md`（v1） |

## 要解决的问题

契约规定：**平台能算的算完，算不了的显式标出来并给取数指引**。

现在平台完全不知道"哪些类来自 API"，所以：

| 场景 | 现在 |
| --- | --- |
| 查询打到 API 源的类 | 返回**空结果**（⛔ Agent 会以为"没有数据"） |
| TTL 里 | 不标 `sourceKind`（Agent 写查询前不知道） |
| catalog/describe | 不说"这个类该走哪个入口" |

## 要做四件

### 1 · TTL 输出 `ontok:sourceKind "api"`

| 规则 | 说明 |
| --- | --- |
| **只有 api 类写** | 库源默认 db，**不写**（客户视图保持干净） |
| **客户视图与建模视图都写** | 这是"数据不在这套本体里"的事实，不涉端点/凭据 |
| ⛔ 不写任何接口细节 | 端点、参数、凭据变量都只在 access 契约里 |

### 2 · 查询结果输出 `@pending` + `meta.complete`

JSON-LD 模式：

```json
{ "@graph": [...],
  "@pending": [ { "@id": "ontok:customer/7d49…", "@type": "ontok:customer",
                  "ontok:sourceKind": "api",
                  "reason": "source_is_api",
                  "get": { "class": "customer", "filter": {"ids":["7d49…"]},
                           "pushdown": ["ids"], "ref": "access#Customer@v2" } } ],
  "meta": { "complete": false, "unresolved_classes": ["customer"], "mapping_version": 2, ... } }
```

rows 模式（没有图结构可挂 pending）：

```json
{ "rows": [...], "meta": { "complete": false, "unresolved_classes": ["customer"] } }
```

⛔ rows 模式**不许**静默返回空表——必须让调用方知道"少了一块"。

### 3 · `query serve --access-mapping <file>`

| 行为 | 说明 |
| --- | --- |
| catalog / describe | 把 api 类也列出来，带 `access: {kind, queryable_by, pushdown_fields, freshness}` |
| SPARQL 打 api 类（jsonld 模式） | `@graph: []` + `@pending` 一条 + `complete:false` |
| SPARQL 打 api 类（rows 模式） | **明确报错** `NOT_DB_SOURCE` + hint（⛔ 不返回空表） |
| 跨源边 | **不下发 `get`**（规则 4）；遍历它 ⇒ `CROSS_SOURCE_UNSUPPORTED` |

### 4 · 两个错误码

`NOT_DB_SOURCE` / `CROSS_SOURCE_UNSUPPORTED`，各带可行动 hint。

## 不做

- ⛔ 不执行 API 调用（凭据不出客户边界）
- ⛔ 不做跨源 join / 联邦
- ⛔ 不回填（Agent 取到的数据不写回）
- ⛔ 不改 `access` 契约本身的格式（那是 REQ-144）
- ⛔ 不引依赖

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | **不给 `--access-mapping` ⇒ 行为与现在逐字节相同**（零回归） |
| R2 | `meta.complete` 与 `@pending` **必须一致**（有 pending ⇒ complete=false；无 ⇒ true） |
| R3 | `@pending` **不进 `@graph`** |
| R4 | 跨源边**不下发 `get`**，遍历报 `CROSS_SOURCE_UNSUPPORTED` |
| R5 | rows 模式遇 api 类 ⇒ **报错**（不是空表） |
| R6 | 客户侧部署**不加载映射** ⇒ 自然拿不到 `get`（不做额外的身份裁剪逻辑） |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | `complete:false` 被下游忽略 ⇒ 半个答案 | 契约里写死"必须先处理 pending"；skill 里加禁忌 |
| 2 | 映射与本体版本漂 | `meta.mapping_version` + `get.ref@v` |
| 3 | api 类混进 SPARQL 让查询语义变复杂 | 明确：**一个查询只能落在库源**；api 部分只能 pending |

## 交付物

1. TTL `sourceKind`（仅 api）+ 无映射时零变化
2. `@pending` + `meta.complete` + `mapping_version`
3. `--access-mapping` + catalog/describe 的 `access` 字段
4. 两个错误码
5. 已知限制
