# ontok 技能包 · 两端四份 skill

> ⭐ **本目录是 skill 的唯一存放处**（2026-10-26 从 `Deploy/skills/` 收拢过来）。
> 改 skill 只改这里；`Deploy/` 不再保留副本。

---

## 一 · 两端四份

```
构建端（FDE）                      消费端（数字员工 / Axon）
├─ ontok-build   造本体            ├─ ontok-query         怎么用
└─ ontok-extract 抽取契约          └─ <项目>-ontology      这套里有什么
```

| skill | 端 | 讲什么 | 换项目要不要改 |
| --- | --- | --- | --- |
| **`ontok-build`** | 构建 | 用文档/库**造本体**：摄取 → 提案 → 人签 → 冻结 → 导出 → 发布。含「失败长相」10 条 | ❌ 通用 |
| **`ontok-extract`** | 构建 | **通用抽取契约**：什么算概念/关系、证据怎么给、⛔ 禁泛动词。**不规定输出格式**（由调用方运行时指定） | ❌ 通用 |
| **`ontok-query`** | 消费 | **流程**：看 `health.mode` → 取 TTL（地图）→ catalog/describe → 判断能否查 → 读回执（`@pending`/`complete`）→ 转达限制 | ❌ 通用 |
| **`ops360-ontology`** | 消费 | **内容**：这套部署里 20 个类、10 条关系、10 条口径、哪些必须拒绝 | ✅ 换项目就换这份 |

**为什么这样分**：

> - **构建 vs 消费**：造本体的人和用本体的人，看的东西不一样。
> - **流程 vs 内容**：流程是产品能力（跨项目通用）；概念清单是项目资产（一项目一份）。

---

## 一·五 · 四份 skill 的关系

```
ontok-build ──(需要抽取时)──▶ ontok-extract ──▶ 产出断言/提案 ──▶ 人签 ──▶ 本体（TTL）
                                                                        │
ontok-query ◀────────────────── 数字员工按 TTL + catalog 使用 ◀──────────┘
     ▲
ops360-ontology（这套部署的具体内容，与 ontok-query 配套）
```

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
