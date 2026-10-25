# 能力与架构全景（`develop` `6f4987d`，2026-10-25）

> 按**能力**组织，不按需求编号。需求号只作溯源。
> ⚠️ 本文件是"当前能力"的**唯一出处**；协议形状见 `BA/design/`，使用步骤见对应 skill。

---

## 一 · 一句话架构

> **本体说"世界由什么构成"；两条路径分别回答"数据在哪、怎么取"。**
> 关系库由**平台执行**（R2RML）；云 API 由**客户侧执行**（access mapping）。
> 平台永远只读、无状态、不碰客户凭据。

```
                 ┌────────────── 本体（TTL）──────────────┐
                 │  类 / 属性 / 关系 / 口径               │
                 │  ontok:sourceKind "api" ⇒ 数据不在这   │
                 └───────────────┬───────────────────────┘
                                 │
        ┌────────────────────────┴─────────────────────────┐
        │                                                  │
   路径 A：关系库                                       路径 B：云 API
   R2RML 映射                                          access mapping 契约
   ↓                                                   ↓
   **平台执行**（query serve 翻 SQL + 注入租户）          **客户侧执行**（Agent 自己调）
   ↓                                                   ↓
   直接返回数据                                         返回 @pending + get/ref + local_hint
        │                                                  │
        └──────────────┬───────────────────────────────────┘
                       ▼
              Agent 拼装最终答案
        （`meta.complete` 说明是否还缺一块）
```

---

## 二 · 两条取数通路（**这是今天最大的变化**）

| | 路径 A · 关系库 | 路径 B · 云 API |
| --- | --- | --- |
| 映射 | **R2RML**（标准） | **access mapping**（RML + `ontok:` 扩展） |
| 谁执行 | **平台**（`query serve`） | **客户侧**（Agent / `ontok access fetch`） |
| 凭据在哪 | 平台（只读账号） | **客户侧**（AK/SK 不出边界） |
| 查询语言 | SPARQL v1（翻成 SQL） | **不是 SPARQL**：按类取数 + 声明式能力 |
| 缺数时 | —— | 返回 `@pending` + 取数指引 |
| 跨源 | 单源 | 单源；**跨源 join 谁都不做** |

**为什么 API 不给 SPARQL**：给了就等于承诺"能下推"，而多数云 API 只支持很少的过滤参数 ⇒ 只能全表扫。**能力边界必须一眼可见**。

---

## 三 · 摄取与建模

| 能力 | 入口 | 边界 |
| --- | --- | --- |
| 源库勘查 | `ontok-introspect --out schema.json` | 只读 `pg_catalog`；**只支持 PostgreSQL**（多方言见 REQ-146，未排期） |
| 表名/属性名提案 | `ontok name suggest` | 提案不权威，需人签；`--names` 现为**必填** |
| 排除表提案 | `ontok exclude suggest` | 同上 |
| 概念分组 | `ontok concept suggest/import/digest` | 算法版不许冒充 AI 版 |
| 关系提案 | `ontok-propose --fk-out` | 值域重叠实测；≥95% strong / 50–95% medium / <50% 否决 |
| 文档摄取 | `ontok doc ingest --input docs/` | 只吃 `.md`/`.txt`；指纹缓存二次 0 token；超预算不产出半份 |
| 文档合并 | `ontok doc merge` | 机械对齐，⛔ 不猜语义 |
| **纯文档通路** | `ontok doc merge --allow-doc-only` | 无库项目从零建实体；key 确定性；band ≤ medium |
| 签字 | `ontok doc sign --by <人>` | 不许匿名；幂等；不就地改 |
| **API 源映射** | `ontok access validate/fetch/publish` | 四条有牙检查；⛔ 阶段 0 不发网络；未签映射拒绝取数 |

---

## 四 · 评审与版本

| 能力 | 入口 |
| --- | --- |
| 提案导入 | `ontok propose import --file` |
| 双轨评审（**唯一权威**） | `ontok review decide --track biz\|data` |
| 评审页面 | `ontok review serve` |
| 应用 | `ontok proposal apply` |
| 版本冻结 | `ontok version freeze` |
| 置信度校准 | `ontok confidence calibrate` |

---

## 五 · 产物

| 产物 | 入口 | 关键约束 |
| --- | --- | --- |
| TTL（客户视图） | `ontok ontology export` | 无列名/租户列/主键；**api 类标 `ontok:sourceKind "api"`** |
| TTL（建模视图） | `--model-view --schema --tenancy` | 加主键、枚举、租户列、证据等级 |
| R2RML | `ontok r2rml generate` | 只有 `true` 边生成 join |
| **access mapping（发布态）** | `ontok access publish` | RML 风格 JSON-LD；`reviewedBy` 未签则不写三元组 |

**TTL 是确定性的**：同一份提案在不同库重建 ⇒ 逐字节相同（已验证）。

---

## 六 · 服务（三个入口，三套凭证）

| 服务 | 端口 | 凭证 | 能力 |
| --- | --- | --- | --- |
| `query serve` | 8800 | query token（绑租户） | SPARQL（**默认 JSON-LD**）/ catalog / describe / TTL 下载 / MCP |
| `console serve` | 8899 | admin token | 项目/评审/冻结/导出/声明/审计/查询台 |
| `portal serve` | 8900 | portal token | 只给预置问题；无 SPARQL |

### 两种部署模式（**决定能做什么**）

| 模式 | 启动参数 | 能 | 不能 |
| --- | --- | --- | --- |
| **普通** | `--schema` + `--policy` + 源库 | 查真数据 | —— |
| **本体模式** | `--ontology-only` | 只有本体（catalog/describe/TTL） | SPARQL 恒 **503 `NO_DATA_SOURCE`** |

### 新增：API 源支持（`--access-mapping`）

| 行为 | 说明 |
| --- | --- |
| TTL | api 类带 `ontok:sourceKind "api"` |
| catalog / describe | 带 `access: {kind, queryable_by, pushdown_fields, freshness}` |
| SPARQL 打 api 类（JSON-LD） | `@graph: []` + `@pending` + `meta.complete:false` |
| SPARQL 打 api 类（rows） | **400 `NOT_DB_SOURCE`**（⛔ 不返回空表） |
| 跨源遍历中点名 api 属性 | **400 `CROSS_SOURCE_UNSUPPORTED`** |
| 不给映射 | **16/16 产物逐字节不变** |

---

## 七 · 前端

| 页面 | 特点 |
| --- | --- |
| 内部工作台（59,988 B） | 壳零数据、token 只进 sessionStorage、查询台显示 warnings/coverage/truncated |
| 客户门户（13,692 B） | 租户锁死、只点预置问题、⛔ 不露表名/列名/SQL |
| 本体图浏览器（`ontok serve`） | 只读；**只允许绑回环**（要对外需改） |

---

## 八 · 三份 skill（**A2A 的交接面**）

| skill | 给谁 | 讲什么 |
| --- | --- | --- |
| `ontok-build` | **FDE** | 用文档/库造本体；含「失败长相」10 条 |
| `ontok-query` | **数字员工** | 怎么用本体服务；含 `@pending` / `complete` 处理规则 |
| `<项目>-ontology` | 数字员工 | 这套部署里有什么、必须拒绝什么 |

**分工原则**：流程（通用）与内容（项目专用）分开 ⇒ 换项目只换后者。

---

## 九 · MCP

| 形态 | 入口 | 内容 |
| --- | --- | --- |
| stdio | `ontok mcp-server` | 冻结快照 |
| HTTP/SSE | `query serve` 的 `/mcp` | 4 工具（catalog/describe/sparql_query/sample）+ 2 资源（TTL 两视图） |

`sparql_query` 的 `format` **默认 `jsonld`**；要表格传 `"rows"`。

---

## 十 · 部署（当前 UAT 实况）

| 项 | 值 |
| --- | --- |
| 集群 | `anspire-k8-admin@anspire-dify` · 命名空间 `anspire-ontok-uat` |
| 镜像 | `ontok:0.1.6` + `cloudflared:2026.10.1`（2/2 Running） |
| 内网 | `https://an-onto.local.anspire.cn`（traefik + TLS） |
| 公网 | `https://an-notol.mic-scfw.cloudns.org`（CF Tunnel，**不过 traefik**） |
| 模式 | `ontology-only` · 项目 `ops360` v2 |
| 现状 | 20 类 / 10 关系 / TTL 21,335 B · `AB0530CF…` |

⚠️ **这套没加载 `--access-mapping`**，所以看不到 `@pending`；要演示混合查询需要**普通模式 + api 类 + 映射**。

---

## 十一 · 明确不做（边界）

| # | 不做 | 理由 |
| --- | --- | --- |
| 1 | 跨源 join（库 ⇄ API） | 没有任何一层能执行；Agent 分段拼 |
| 2 | 联邦执行器 | 那是 Trino/DuckDB 的领域 |
| 3 | 平台调云 API | 凭据不出客户边界 |
| 4 | 数据回填 | 平台保持无状态 |
| 5 | 多 SQL 方言（暂） | 只支持 PG；SQLite/MySQL 见 REQ-146（已就绪未排期） |
| 6 | 产品内置业务表 | **业务表是数据源的，不是产品的**——否则每接一个业务改一次产品 |
| 7 | 文档↔表的语义自动映射 | 业务判断，猜错会"能跑但不对" |

---

## 十二 · 未完成 / 待排

| 项 | 状态 |
| --- | --- |
| access mapping **阶段 1**（真 API + TC3 签名） | 需只读 AK/SK + 一份脱敏真响应 |
| 跨源边带 `get{ids}` | 阶段 2（需"只读 child 列"路径） |
| 混合查询**部分求值** | 阶段 2（现为整体 pending） |
| REQ-146 多方言 | 已就绪未排期 |
| 本体图浏览器对外 | 需去掉回环限制 |
| `/metrics` 公网拦截 | **需在 CF 面板加 WAF 规则**（当前公网可访问） |

---

## 十三 · 代码里能查到的事实

```
lib 测试            756 passed（另有环境依赖红，需 ONTOK_TEST_DATABASE_URL）
出口登记            63 条
TTL                客户视图 293,814 B / 建模视图 371,356 B（anspire 项目）
R2RML              84 TriplesMap / 70 joinCondition
镜像               ontok 328 MB（含 bun + pi，供抽取）
```
