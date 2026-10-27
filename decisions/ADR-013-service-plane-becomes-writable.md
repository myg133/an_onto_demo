# ADR-013 · 服务面首次可写：构建包导入端点落在 `query serve`

- 状态：**已决**（用户 2026-10-27 13:0x 确认「建项目/上传/解析/激活 这些在 studio 里呀」）
- 取代/补充：ADR-010（Studio 挂在 `query serve`）、REQ-152（二进制拆分）
- 影响：REQ-159 的 8 条端点**从 console 迁到 `query serve`**

---

## 一 · 背景与一个被搞错的方向

REQ-159（构建包导入）的 8 条端点最初实现在 **`console`**（`src/console/`）。
而 `console` 的 CLI 入口在 **`ontok-builder`** 里 —— **构建面，不部署到服务器**。

⇒ 于是出现一个荒谬的结论：**"在服务端建项目、上传包" 的能力，部署不出去。**

**根因（我的失误）**：我看到 `src/console` 是库代码、且它管的是"服务端的项目与版本"，就假定它该跑在服务端，
**没有先问"这 8 条最终在哪个 UI 上被点"**。用户指出：这些操作在 **Studio** 里（`/studio`，挂在 `query serve`）。

---

## 二 · 决定

**那 8 条端点挂到 `query serve`**，由 Studio 调用：

| # | 端点 | 说明 |
| --- | --- | --- |
| 1 | `POST /api/projects` | 建项目 + 发 key |
| 2 | `POST /api/projects/:p/bundles` | 上传（octet-stream） |
| 3 | `GET  /api/projects/:p/bundles/:id` | 包状态 |
| 4 | `PUT  /api/projects/:p/database` | 配 DSN |
| 5 | `POST /api/projects/:p/database/test` | 测连通 |
| 6 | `POST /api/projects/:p/bundles/:id/import` | 解析入库 |
| 7 | `POST /api/projects/:p/bundles/:id/activate` | 冻结 + 生效 |
| 8 | `GET  /api/projects/:p/key` | 查看 key（写审计） |

**部署形态不变**：仍然只有 `ontok`（`query serve`），**不需要新进程、不需要部署 builder**。
只需新增一个环境变量 `ONTOK_SECRET_KEY`（DSN 与 query key 的加密列）。

---

## 三 · 代价：服务面从「只读」变「可写」

这是 ADR-010 之后**最大的一次性质变化**，必须写死边界：

| 边界 | 规则 |
| --- | --- |
| **谁能写** | 只有 **`kind=admin`** 的凭据。数字员工的 query key 调这 8 条 ⇒ **403 `key.not_admin`** |
| **读路径不变** | catalog / describe / sparql / TTL / MCP 的行为与鉴权**一个字节不变** |
| **写的是什么** | 只写**元数据库**（projects / versions / snapshots / keys / DSN 密文）；⛔ 不碰客户源库（源库只在 `database/test` 时连一次，只读、只 `SELECT version()`） |
| **审计** | 每个写端点写一行（含失败）；⛔ 不记 key/DSN 明文 |
| **失败语义** | 解析在单事务里，失败 ⇒ 零残留（REQ-159 AC-006 已验证） |

### admin 凭据怎么来（**新**）

`query serve` 现在的 token 表**没有 kind**：

```rust
pub struct TokenEntry { token, digest, project, tenant }   // 分不出 admin 与数字员工
```

⇒ 必须加 `kind: admin | query`：

| 项 | 规则 |
| --- | --- |
| `kind=query`（数字员工） | 必须有 `project`（多项目模式）；**不能**调 8 条写端点 |
| `kind=admin`（FDE） | **必须没有 `project`**（admin 不绑项目，与 console 同一规则） |
| 缺 `kind` | 视为 `query`（向后兼容现有 token 表） |

---

## 四 · 为什么不是另外两条路

| 备选 | 为什么否 |
| --- | --- |
| 部署 `ontok-builder` 到服务器 | builder 是 FDE 本地工具（抽取/打包/审核），**它不该在生产环境里有一份**；且会带来整套构建依赖 |
| 服务端再加一个"收件 API"新进程 | 部署面变大；而 Studio 已经在 `query serve` 上，**多一个进程只为 8 条端点不划算** |

---

## 五 · 迁移成本（已核，比想象中低）

| 层 | 现状 | 迁移动作 |
| --- | --- | --- |
| 业务逻辑 `src/console/provision.rs` | **零引用 `ConsoleService`**，全是 `(pool, cipher, …)` | ✅ **不动**，直接在 `query serve` 里调 |
| 包解析 `src/console/bundle.rs` | 纯函数 | ✅ 不动 |
| 加密 `src/console/dsn.rs` | 纯函数 | ✅ 不动 |
| 路由 `src/console/http.rs` | 8 条 handler 用 `State<Arc<ConsoleService>>` | 搬到 `src/query/http.rs`，state 换成 query serve 的 |
| 前端 | console 页面的「构建包」tab（另一 BA 做的） | **撤掉/标记已迁**；改在 `src/studio/studio.js` 里做 |

---

## 六 · 后果清单（谁要改什么）

| 方 | 要做的事 |
| --- | --- |
| **join（我）** | 8 条路由搬到 `query::http`；`query::tokens` 加 `kind`；`ONTOK_SECRET_KEY` 在 `query serve` 启动时 fail-fast；Studio UI；ADR + 契约 |
| **lion** | console 页面的「构建包」tab **撤掉**（那套 JS 可以留着当 Studio 的参考实现）；REQ-160 的 `review open` 不受影响（它用的是 console，不是这 8 条） |
| **Deploy** | 只加 `ONTOK_SECRET_KEY`；console 仍然不部署 |
| **Studio** | 加「建项目 / 上传 / 解析 / 激活 / 查看 key」—— 静态样机 `BA/design/bundle-prototype` 即目标形态 |

---

## 七 · 待确认的边界（我按这个实现，有异议现在说）

1. **Studio 匿名可见**：`/studio` 页面壳免 token（现状）；但**建项目等操作要求页面上先输入 admin key**（存 sessionStorage，与现有 Studio 的 token 处理一致）。⇒ 匿名能看到页面，但点"＋"会被要求先输 admin key。
2. **admin key 从哪来**：运维生成（`openssl rand -hex 32` 之类）写进 `query-tokens.json` 的 `kind=admin` 条目，**不进页面、不进 URL**。
3. **多项目模式下 admin 不绑项目**：admin 可以操作**任意项目**（这是它存在的理由）；审计里记 `project=` 目标。
