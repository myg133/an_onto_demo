# ontok 技能包 · 三份**通用** skill

> ⭐ **本目录是 skill 的唯一存放处**（2026-10-26 从 `Deploy/skills/` 收拢）。
> 改 skill 只改这里；`Deploy/`、`code/` 里**不许**出现副本。

> ⛔ **本目录只放通用 skill。**
> **项目专用内容**（"这套部署里有哪些类、哪些必须拒绝"）**不随产品交付** ——
> 它由 FDE 按 `ontok-build` 产出、随该项目的部署单独发布。

---

## 一 · 三份，两端

```
构建端（FDE）                       消费端（数字员工 / Axon）
├─ ontok-build    造本体            └─ ontok-query   怎么用本体服务
└─ ontok-extract  抽取契约
```

| skill | 端 | 讲什么 | 换项目改不改 |
| --- | --- | --- | --- |
| **`ontok-build`** | 构建 | 用文档/库**造本体**：摄取 → 提案 → 人签 → 冻结 → 导出 → 发布。含「失败长相」10 条 | ❌ 通用 |
| **`ontok-extract`** | 构建 | **通用抽取契约**：什么算概念/关系、证据怎么给、⛔ 禁泛动词。**不规定输出格式**（由调用方运行时指定） | ❌ 通用 |
| **`ontok-query`** | 消费 | **流程**：看 `health.mode` → 取 TTL（地图）→ catalog/describe → 判断能否查 → 读回执（`@pending` / `complete`）→ 转达限制 | ❌ 通用 |

**为什么这样分**：

> **构建 vs 消费**：造本体的人和用本体的人，看的东西不一样。
> **通用 vs 项目**：产品只交付"怎么干活"；"这里具体有什么"是项目资产。

---

## 二 · 安装

```
<agent-skills-dir>/
├── ontok-build/SKILL.md
├── ontok-extract/SKILL.md
└── ontok-query/SKILL.md
```

**数字员工至少要装 `ontok-query`**；FDE 装 `ontok-build` + `ontok-extract`。

⚠️ 装 `ontok-query` 但**没有本项目的概念清单**时，agent 会先 `catalog` + 取 TTL 自己读——
**能干活，但慢**。要不要给每个项目配一份内容说明，由部署方决定（不属本目录）。

---

## 三 · 连接信息（**必须由平台注入**）

⛔ skill 里**没有地址和 token**，也不许加。两个值由部署方提供：

| 变量 | 例 |
| --- | --- |
| `ONTOK_BASE_URL` | `https://<服务地址>` |
| `ONTOK_TOKEN` | `PARTNER-…`（**查询**口径，不是管理口令） |

### 方式 A：MCP server 配置（推荐）

```json
{
  "mcpServers": {
    "ontok": {
      "type": "http",
      "url": "${ONTOK_BASE_URL}/mcp",
      "headers": { "Authorization": "Bearer ${ONTOK_TOKEN}" }
    }
  }
}
```

⚠️ 三个坑（都踩过）：

| # | 坑 | 对的样子 |
| --- | --- | --- |
| 1 | `type` 写成 `"sse"` | **`"http"`**（服务端主用法是 streamable HTTP） |
| 2 | `headers` 写成数组 `[{name,value}]` | **对象** `{"Authorization": "..."}`（数组是 ACP/pi 那套的形状） |
| 3 | `url` 只写到域名 | 必须带 **`/mcp`** |

### 方式 B：环境变量

平台支持注入环境变量时，skill 会按上面的变量名去读。

### 30 秒自测（不依赖客户端）

```bash
curl -X POST "$ONTOK_BASE_URL/mcp" \
  -H "Authorization: Bearer $ONTOK_TOKEN" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

**返回 4 个工具**（`catalog` / `describe` / `sparql_query` / `sample`）⇒ 接口与凭据都好，问题在客户端配置。

---

## 四 · 平台会看到的四个工具

| 工具 | 参数 | 用途 |
| --- | --- | --- |
| `catalog` | `page`、`page_size` | 浏览类/属性/关系 |
| `describe` | `name` | 单个概念详情（含 `access.kind`：`db` 走 SPARQL / `api` 走 fetch） |
| `sparql_query` | `query`、`max_rows`、**`format`**（`jsonld` **默认** / `rows`） | 查数据；**默认返回 JSON-LD 图，要表传 `format:"rows"`** |
| `sample` | `name`、`limit` | 取样（需真连库，本体模式下恒 503） |

资源：`ontok://ontology/customer.ttl`、`ontok://ontology/model.ttl`

**⚠️ 两种部署模式的能力不同**（以 `GET /api/health` 的 `mode` 为准）：

| mode | 能查数据？ | `sparql_query` 行为 |
| --- | --- | --- |
| 普通（有源库） | ✅ | db 类给数据；api 类给 `@pending` |
| `ontology-only` | ❌ 不碰源库 | db 类 503 `NO_DATA_SOURCE`；**api 类给 `@pending` + 取数指引** |

---

## 五 · 交付清单

| 文件 | 内容 |
| --- | --- |
| `skills/ontok-build/SKILL.md` | 构建端：造本体（通用） |
| `skills/ontok-extract/SKILL.md` | 构建端：抽取契约（通用） |
| `skills/ontok-query/SKILL.md` | 消费端：怎么用本体服务（通用） |
| `README.md` | 本文件 |
| `mcp.example.json` | MCP 配置模板 |

---

## 六 · 换一个新项目怎么办

1. 用 `ontok-build` 造本体：摄取 → 提案 → 人签 → 冻结
2. （可选）给这个项目写一份「这套部署里有什么」的说明 —— **放在该项目的交付物里，不放本目录**
3. `ontok-query` / `ontok-build` / `ontok-extract` **都不用动**
4. 平台侧只改 `ONTOK_BASE_URL` / `ONTOK_TOKEN`
