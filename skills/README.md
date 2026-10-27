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
| **`ontok-build`** | 构建 | 用文档/库**造本体**：摄取 → 草稿 → 人签 → 定版 → 导出构建包 →（人工）传 Studio。含「失败长相」10 条 | ❌ 通用 |
| **`ontok-extract`** | 构建 | **通用抽取契约**：什么算概念/关系、证据怎么给、⛔ 禁泛动词。**不规定输出格式**（由调用方运行时指定） | ❌ 通用 |
| **`ontok-query`** | 消费 | **流程**：看 `health.mode` → 取 TTL（地图）→ catalog/describe → 判断能否查 → 读回执（`@pending` / `complete`）→ 转达限制 | ❌ 通用 |

> ⭐ **API 源的取数不要自己写**：AM 里的 `sources`/`entities` 已给出 action、分页、pushdown 与字段映射 —— 直接用 [`ontok-query/access_fetch.py`](ontok-query/access_fetch.py)（零第三方依赖），跑法与 9 个错误码见 [`ontok-query/SKILL.md`](ontok-query/SKILL.md) §4.5。

**为什么这样分**：

> **构建 vs 消费**：造本体的人和用本体的人，看的东西不一样。
> **通用 vs 项目**：产品只交付"怎么干活"；"这里具体有什么"是项目资产。

---

## 一·五 · 两种运行模式 ↔ 该加载哪份 skill

> ⚠️ **事实先摆清**：抽取契约**只有一份**（`ontok-extract`），两种模式**共用**。
> 差别只在「**谁把这份文件交到 agent 手里**」和「**谁去起评审台**」。
> `ontok-build` 是 **FDE 的流程手册**（人在跑，不是 agent 在跑），两种模式下**都是人读**。

| 模式 | 谁驱动 | skill 怎么进 agent | 抽完之后谁起评审台 | 该加载什么 |
| --- | --- | --- | --- | --- |
| **ACP 模式** | `ontok-builder build run --agent acp --agent-command <cli> --agent-skill <path>` **主动驱动**外部 agent | 由 `--agent-skill` 把**文件路径**传给对端（agent 自己读） | **工具自动**起（四段跑完直接进评审台，**无口令**） | `ontok-extract` → 对端 agent；`ontok-build` → FDE |
| **独立 agent 模式** | 用户自己开 Claude Code / Codex / 其它 agent | 用户把 `ontok-extract/SKILL.md` **装进 agent 的 skills 目录**（或直接贴进上下文） | **用户手动**跑 `ontok-builder console serve --dir <工作目录>` | `ontok-extract` → 该 agent；`ontok-build` → FDE |

三条最容易搞错的地方：

| # | 误解 | 事实 |
| --- | --- | --- |
| 1 | 「两种模式要两份契约」 | ❌ 契约一份。差别只在**谁分发**（工具传路径 / 人装目录） |
| 2 | 「`ontok-build` 是给 agent 读的」 | ❌ 它是**人**的流程手册；agent 读的是 `ontok-extract` |
| 3 | 「抽完就生效」 | ❌ 抽出来的是**草稿**；人逐条签 → **定版** → 导出的是**定版** |

时序图与「谁负责什么」：[`MODES.md`](MODES.md)。

---

## 二 · 安装

```
<agent-skills-dir>/
├── ontok-build/SKILL.md
├── ontok-extract/SKILL.md
└── ontok-query/SKILL.md
```

**数字员工至少要装 `ontok-query`**；FDE 装 `ontok-build` + `ontok-extract`。

**装给谁、谁来装**取决于运行模式（见 §一·五）：

| 模式 | 这些文件装到哪 |
| --- | --- |
| ACP 模式 | **不用手工装**：`--agent-skill <ontok-extract/SKILL.md>` 在运行时把路径交给对端 |
| 独立 agent 模式 | 人把 `ontok-extract/SKILL.md` 放进该 agent 的 skills 目录；`ontok-build` 留在 FDE 自己手上 |

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
| `MODES.md` | 两种运行模式：谁加载什么、谁起服务、谁签字 |
| `README.md` | 本文件 |
| `mcp.example.json` | MCP 配置模板 |

---

## 六 · 换一个新项目怎么办

1. 用 `ontok-build` 走完一条链：**摄取 → 草稿（`out/proposal.json`）→ 人逐条签 → 定版（`.ontok/versions/vN/`）→ 导出构建包 →（人工）传 Studio**
2. （可选）给这个项目写一份「这套部署里有什么」的说明 —— **放在该项目的交付物里，不放本目录**
3. `ontok-query` / `ontok-build` / `ontok-extract` **都不用动**
4. 平台侧只改 `ONTOK_BASE_URL` / `ONTOK_TOKEN`
