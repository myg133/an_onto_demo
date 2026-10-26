# REQ-155 · UI 重组：删 Portal · Studio 落地 · Console 重设计

- 状态：**已就绪**
- 提出：2026-10-26
- 关联：ADR-010（Studio 开放的安全边界）、REQ-125（Console）、REQ-126（Portal，本次删除）、REQ-093（serve，本次改造）

---

## 一 · 一句话

**产品只留两个界面：`console`（构建/管理，FDE 用）和 `studio`（本体与查询，客户看）。**

---

## 二 · 为什么

| 现象 | 事实 |
| --- | --- |
| 线上没有任何 UI | UAT 只跑 `query serve`；`/console`、`/portal`、`/studio` **全部 404**（实测） |
| Portal 从没被用过 | 要求"预置问题 + 租户口令 + DB 库源"；我们唯一在跑的项目是**纯 API 本体**，门户查了只会 503 |
| 三个界面维护不动 | console(4.3K) + portal(3.5K) + serve 页，三套 HTML/JS，各自的鉴权模型 |
| 视觉不统一 | serve 的页面是 2016 风格；我们的新 UI（`BA/design/ui-prototype/`）已确认 |

**用户决定**：Portal 砍掉；Studio 进服务面直接开放；Console 重新设计。

---

## 三 · 范围

### ① 删除 Portal（硬删）

| 项 | 内容 |
| --- | --- |
| 代码 | `src/portal/` 8 文件 / **3,464 行** 全删；`lib.rs` 的 `pub mod portal;` |
| 命令 | `ontok-builder portal serve` 整条删除 |
| 测试 | `tests/test_req126_portal.rs`(883 行) + `tests/test_req126_live.rs` 删除 |
| 引用 | 全仓 **287 处 / 18 文件** 清理（文档、注释、help 文本同步） |
| Deploy | `k8s/base/deployment-portal.yaml` + `compose/` 里的 portal 服务与凭据文件 |
| **记录** | 删前**打 tag**（`before-req155-portal-removal`），将来要恢复用 `git revert` |

**⛔ 不保留死代码**：不留"注释掉"、不留 `#[cfg(feature)]`。

### ② Studio 落地（服务面）

| 项 | 内容 |
| --- | --- |
| 位置 | `code/src/studio/`（`mod.rs` + 三个静态文件，`include_str!`） |
| 路由 | `GET /studio` → HTML；`GET /studio/studio.css`、`/studio/studio.js` |
| 数据 | **同源相对路径**（`/api/catalog`、`/api/describe`、`/api/sparql`）⇒ ⛔ 无 CORS |
| 鉴权 | 页面壳免口令；`/api/*` 仍要 Bearer（与 `query serve` 同口径） |
| 开放 | **挂 `query serve`**（已公开）；`ontok serve` 的回环限制**不动**（见 ADR-010-A） |
| 替代 | `ontok serve` **保持原样**（本机只读工具，回环 + GET-only 不动）；Studio 不用 `/api/graph` |

### ③ Console 重设计（**先出视觉稿，不动代码**）

| 项 | 内容 |
| --- | --- |
| 目标 | 视觉与 Studio 一致（同一套 design token） |
| 视图 | 仍是 7 个：项目与版本 / 评审 / 导出 / 声明编辑 / 审计 / 查询台 / 目录浏览 |
| 产出 | 静态 HTML 样机（放 `BA/design/console-prototype/`），用户确认后再改代码 |
| 约束 | ⛔ 不改 console 的后端端点与 7 条安全断言 |

---

## 四 · 验收（摘要）

| # | 判据 |
| --- | --- |
| AC-001 | `src/portal/` 不存在；`grep -ri portal src/ tests/` 只剩**历史说明**（0 处代码引用） |
| AC-002 | `ontok-builder portal` ⇒ `unrecognized subcommand`；四个 bin 的 `--help` 里没有 portal |
| AC-003 | 测试数**下降**且下降量 = 删除的 portal 用例数（逐条列出），**不许把失败改 SKIP** |
| AC-004 | `GET /studio` ⇒ 200 HTML；`/studio/studio.css`、`/studio/studio.js` ⇒ 200 且 Content-Type 正确 |
| AC-005 | Studio 页面在浏览器里**能出图**（30 类 / 34 关系），点节点能看属性 |
| AC-006 | **C1**：`POST /studio`、`PUT`、`DELETE` ⇒ **405** |
| AC-007 | **C2/C4**：`/api/catalog` 无口令 ⇒ **401** 且响应体不含任何本体数据 |
| AC-008 | **S1**：`/studio` 只接受 GET，`POST/PUT/DELETE` ⇒ **405** |
| AC-008b | **S2**：`/studio` 返回的 HTML **不含任何本体数据**（grep 类名 = 0） |
| AC-008c | **S3/S4**：无口令 ⇒ 页面不显示数据、`/api/*` 得 401 |
| AC-009 | **C5**：客户视图的脱敏闸仍生效（748 条说明仍被隐去） |
| AC-010 | 服务面 8 case 里 **health/catalog/describe/503/TTL×2 不变**；`get` 仍 4/10 键 |
| AC-011 | 镜像里仍**只有** `ontok` 一个 bin；`strings ontok \| grep -c X-TC-Action` = 0 |

---

## 五 · 分期（一次派单太大）

| 期 | 内容 | 依赖 |
| --- | --- | --- |
| **P1** | 删 Portal（含 Deploy） | 独立，**现在就能做** |
| **P2** | Studio 挂 `query serve`（静态资源 + `/studio` 路由） | ADR-010-A；**不动 `ontok serve`** |
| **P3** | Console 重设计 | 依赖用户确认视觉稿（我先出） |

---

## 六 · 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | **开放只读端点 = 新的攻击面** | ADR-010 的 C1–C6，一条不许少 |
| 2 | 删 Portal 时误删共用代码 | 已确认 `console` 对 portal 引用 **0 处**（完全独立） |
| 3 | 测试数下降被误判成"删测试过关" | AC-003 要求**逐条列出**删了哪些、为什么 |
| 4 | Studio 的 47 KB JS 塞进 Rust 常量不可维护 | 用 `include_str!` 分开三个文件，**不进 Rust 字符串** |
