# Studio · 本体图页面（REQ-155 P2）

> 一句话：**`ontok query serve` 启动后，浏览器打开 `/studio` 就能看本体、点节点看属性、写 SPARQL。**
> 页面壳是编译期常量（零数据），数据全部经既有 `/api/*` 取 —— 没有新增任何端点。

---

## 1 · 怎么打开

服务本来就是 `query serve`（UAT 上已经在 ingress 后面），所以**部署零改动**：

```bash
ontok query serve --project <项目> --version <已冻结版本> --tokens tokens.json \
    --bind 127.0.0.1:8899        # 其余参数与以前完全一样
```

浏览器打开：

| 地址 | 内容 |
| --- | --- |
| `http://<host>:8899/studio` | 页面（也可写 `/studio/`） |
| `http://<host>:8899/studio/studio.css` | 样式表（`text/css`） |
| `http://<host>:8899/studio/studio.js` | 脚本（`application/javascript`） |

三条路由都是 **GET-only**：`POST/PUT/DELETE` 一律 **405**；响应带
`Content-Security-Policy` / `X-Content-Type-Options: nosniff` / `Referrer-Policy: no-referrer`。

---

## 2 · 口令从哪来

- 口令就是 `query serve --tokens tokens.json` 里那条 **`token`**（与 `curl -H "Authorization: Bearer …"` 用的是同一个值）。
- 拿不到就问部署方（运维）。**页面上没有任何默认口令、也没有示例口令**。
- 填进顶栏的口令框、点「连接」后，它只写进 **`sessionStorage`**：
  - 关掉标签页即失效；同一标签页刷新仍在（不用重填）；
  - ⛔ 不进 URL（不写进历史 / 日志 / Referer）、⛔ 不进 `localStorage`、⛔ 不回填到输入框；
  - 点「退出」立即清掉并回到未连接状态。
- 口令错/缺 ⇒ `/api/*` 返回 **401**，页面只显示「需要口令 / 口令无效」，**一个字段都不渲染**。

---

## 3 · 页面能干什么

| 视图 | 用的端点 | 做什么 |
| --- | --- | --- |
| **本体图** | `GET /api/catalog`（分页到 `has_more=false`）、`GET /api/health` | 类 = 节点、关系 = 边（按 `evidence_level` 上色/虚线），拖拽/缩放/搜索；点节点看定义、属性、关系与证据等级 |
| 详情抽屉 | `GET /api/describe?name=:类` | 属性（类型/枚举）、关系、租户口径、数据源（`access.kind`） |
| **目录** | `GET /api/catalog` | 每个类一张卡片：业务名、本地名、属性/关系数量、可查性 |
| **查询台** | `POST /api/sparql` | 执行一条只读 SELECT；回执默认 **JSON-LD**（`@context`/`@graph`/`summary`/`meta`） |

两个细节值得知道（页面会明说）：

- **API 源类**（`access.kind = "api"`）：平台**不执行**查询，回执里的 `@pending` 是**取数指引**
  （`get.class` / `get.filter.ids` / `get.pushdown` / `get.ref`），不是空结果。
  `@pending[].get` 的键集合由映射契约的 `meta.contract_version` 决定（v1 = 4 键，v2 = 10 键）。
- **`meta.complete = false`**：这份结果**不完整**，不能直接当答案。
- 本体模式（`--ontology-only`）下库源类的查询是 **503 NO_DATA_SOURCE**（重试无效），页面显示服务端给的原话。

---

## 4 · 页面**不**干什么（与 ADR-010-A 的 S1–S6 一一对应）

| 不做 | 为什么 / 后果 |
| --- | --- |
| **不写**。任何 `POST/PUT/DELETE` 到 `/studio` 都是 405，页面本身也没有写能力 | S1：它是一张贴在只读服务上的静态墙报 |
| **不留口令**。只有 `sessionStorage`，没有 URL / `localStorage` / cookie / 代码里的默认值 | S3：防 Referer、代理日志、共享机器 |
| **壳不含数据**。`index.html` / `studio.css` / `studio.js` 是编译期常量，五个真实类名与 `ONTOK_SNAPSHOT` 的 grep 都是 **0** | S2：壳可以匿名拿，拿了也只是一张空表格 |
| **不加端点**。只调 `health` / `catalog` / `describe` / `sparql`；没有 `/api/graph` | ADR-010-A：图由前端从 catalog 自己算 |
| **不改鉴权**。`/api/*` 的 Bearer 语义一个字节都没动；页面壳在门之外（`needs_auth` 只认 `/api` 与 `/mcp`） | S4 / S6 |
| **不动脱敏闸**。客户视图的 TTL / catalog 字段与改动前逐字节相同 | S5 |
| **不带离线快照**。设计稿的 `snapshot.js`（460 KB 真实数据）不在产品里 | R6 |

---

## 5 · 页面坏了先看这三件事

1. **401** ⇒ 口令不对或过期（`tokens.json` 里可能根本没有这个值）。
2. **加载失败：HTTP 404** ⇒ 服务版本太旧（这个镜像里没有 `/studio`），不是口令问题。
3. **页面一片空白但 200** ⇒ 浏览器控制台里若报 CSP 违规，说明有人往 HTML 里加了内联
   `<style>` / `<script>` / `style=` —— 本页 CSP **没有** `'unsafe-inline'`，内联样式会被忽略。
