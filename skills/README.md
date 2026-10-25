# ontok 技能包 · 两份 skill 怎么配合

> 给数字员工平台接入 ontok 本体服务用。
> **流程与内容分开**：一份讲"怎么干活"，一份讲"这套部署里有什么"。

---

## 一 · 两份 skill 的分工

| skill | 讲什么 | 换部署要不要改 |
| --- | --- | --- |
| **`ontok-query`** | **流程**：先看 `health.mode` → 取 TTL（地图）→ catalog/describe 定位 → 判断能否查数据 → 读回执 → 转达限制 | ❌ **不用改**（通用） |
| **`ops360-ontology`** | **内容**：这套 Ops360 部署里的 20 个类、10 条关系、10 条口径、哪些问题必须拒绝 | ✅ 换项目就换这份 |

**为什么分开**：

> 流程是产品能力，跨项目通用；概念清单是项目资产，一个项目一份。
> 混在一起，换项目就得重写流程；或者反过来，流程改了每个项目都得跟。

---

## 二 · 安装

把两个目录放进 Agent 的技能目录：

```
<agent-skills-dir>/
├── ontok-query/SKILL.md
└── ops360-ontology/SKILL.md
```

**两份都要装**。只装 `ontok-query` ⇒ agent 不知道怎么调接口但不知道有什么；
只装 `ops360-ontology` ⇒ 知道有什么但不会干活。

---

## 三 · 连接信息（**必须由平台注入**）

⛔ skill 里**没有地址和 token**，也不许加。两个值由部署方提供：

| 变量 | 例 |
| --- | --- |
| `ONTOK_BASE_URL` | `https://an-onto.local.anspire.cn` |
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
| `describe` | `name` | 单个概念详情 |
| `sparql_query` | `query`、`max_rows`、**`format`**（`jsonld` **默认** / `rows`） | 查数据（**本体模式下恒 503**）；**默认返回 JSON-LD 图，要表传 `format:"rows"`** |
| `sample` | `name`、`limit` | 取样（同上） |

资源：`ontok://ontology/customer.ttl`、`ontok://ontology/model.ttl`

---

## 五 · 这套 Ops360 部署的能力边界（先记住）

```
mode = "ontology-only"
```

| 能 | 不能 |
| --- | --- |
| 回答「有哪些概念、怎么关联、口径怎么算」 | 回答「有多少、多少钱、什么趋势」 |
| 给 TTL（20 类 / 10 关系 / 47 属性 / 10 口径） | 给任何实例数据 |
| catalog / describe | SPARQL（恒 503 `NO_DATA_SOURCE`） |

**判据**：`GET /api/health` 的响应里 `"mode": "ontology-only"`。

---

## 六 · 交付清单

| 文件 | 内容 |
| --- | --- |
| `skills/ontok-query/SKILL.md` | 流程 skill（通用） |
| `skills/ops360-ontology/SKILL.md` | 内容 skill（本部署） |
| `README.md` | 本文件 |
| `mcp.example.json` | MCP 配置模板 |

---

## 七 · 换一个新项目怎么办

1. 跑抽取与评审，冻结一个新版本
2. 复制 `ops360-ontology/SKILL.md` 改成新项目的概念分区与边界
3. `ontok-query/SKILL.md` **不用动**
4. 平台侧只改 `ONTOK_BASE_URL` / `ONTOK_TOKEN`（如果项目在另一套部署）
