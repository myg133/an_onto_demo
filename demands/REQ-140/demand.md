# 需求：REQ-140 · JSON-LD 上下文子图（SPARQL 结果 → RDF 图）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-140 |
| 优先级 | **P0（demo 唯一的空白）** |
| 状态 | 进行中 |
| 基线 | `develop` `600ae76` |
| 触发 | 与数字员工平台（Axon）对接：其接口方向②要求返回**上下文子图（JSON-LD/RDF）**，我们现在返回的是**行** |

## 问题

对方架构（v0.3）里写的是：

> 组装为**上下文子图（JSON-LD/RDF）**返回，供 agent 推理。

我们现在的回执：

```json
{ "columns": ["m","name"],
  "rows": [["https://…/#llm_model/9bab…", "Claude Haiku 4.5"], …],
  "row_count": 2, "truncated": false, "sql_echo": "…",
  "warnings": [], "coverage": [], "join_states": [] }
```

**这是表格，不是图。** Agent 拿到它还得自己拼实体关系。

### 好消息：一半的活已经干完了

实测（`sql_echo`）：

```sql
('https://ontok.local/anspire_r2rml_excl#llm_model/' || t0."id")::text AS "m"
```

**类变量的绑定已经是完整实例 IRI**——不用另拼。

## 要做

### 1 · 三个入口（都是加法，默认行为不变）

| 入口 | 方式 |
| --- | --- |
| REST | `Accept: application/ld+json` **或** `?format=jsonld` |
| MCP | `sparql_query` 增加参数 `format`（`rows`(默认) / `jsonld`） |

⛔ 默认 `format=rows` 的回执**一个字节都不许变**。

### 2 · 输出形状（**扁平 `@graph`，不深嵌套**）

```json
{
  "@context": { "ontok": "https://…/anspire_r2rml_excl#", "xsd": "http://www.w3.org/2001/XMLSchema#" },
  "@graph": [
    { "@id": "ontok:llm_model/9bab…", "@type": "ontok:llm_model",
      "ontok:llm_model__model_name": "Claude Haiku 4.5 (Thinking)" }
  ],
  "summary": { "n": 42 },
  "meta": { "truncated": false, "warnings": [], "coverage": [], "join_states": [],
            "tenant_applied": ["customer_id"], "sql_echo": "…" }
}
```

| 决策 | 选择 | 理由 |
| --- | --- | --- |
| 嵌套 vs 扁平 | **扁平 + `@id` 引用** | 深嵌套 3 跳后爆炸；扁平天然合并同实体 |
| 同实体合并 | **同 `@id` 合并属性** | 这才是"子图"而不是"表" |
| 聚合结果 | `summary` 对象 | 聚合没有实体，塞进 `@graph` 会造假节点 |
| `meta` | **必须保留** | 丢了 warnings 就是"图好看但把限定条件吞了" |

### 3 · 节点 vs 字面量

| 值 | 判定 |
| --- | --- |
| 以 `<项目 base>#<类>/` 开头 | **节点引用** `{"@id": …}` |
| 其它 | 字面量（按模型 datatype 给 `@type` 或原生 JSON 数字/布尔） |

判定优先级：**翻译器给的类型信息**优先，字符串前缀只做兜底。

## 不做

- ⛔ 不改默认回执
- ⛔ 不做 RDF/XML、Turtle 输出（只做 JSON-LD）
- ⛔ 不做上下文（`@context`）的服务端远程引用（内联即可）
- ⛔ 不改 SPARQL v1 语法支持集

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | 默认 `rows` 回执**零字节变化** |
| R2 | **确定性**：同一查询两次请求 ⇒ **逐字节相同**（节点按 `@id` 排序，属性按键排序） |
| R3 | `meta` 必须原样带上 `warnings` / `coverage` / `truncated` / `join_states` / `tenant_applied` |
| R4 | 聚合无实体 ⇒ 进 `summary`，⛔ 不许造假节点 |
| R5 | 空结果 ⇒ `@graph: []`（不是 `null`、不是缺字段） |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 输出顺序不稳定 ⇒ 无法对拍 | R2 |
| 2 | IRI/字面量误判 | 优先用翻译器的类型；前缀兜底 |
| 3 | 变量→谓词映射丢失 | 在翻译阶段就保留映射，不从列名猜 |
| 4 | 大结果集内存翻倍（rows + graph） | `format=jsonld` 时**不回 rows**，避免双份 |

## 交付物

1. 三个入口 + 输出形状
2. `var→谓词` / `var→类` 映射的来源说明
3. 合并、聚合、空结果、IRI 判定各一次实测
4. 确定性（两次 sha256）
5. 默认回执零变化（与基线逐字节对比）
6. 已知限制
