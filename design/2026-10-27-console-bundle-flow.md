# 构建包在 Console 上的落地设计（lion 提案）

- 日期：2026-10-27 · 作者：lion（构建端 BA）· 读者：join（服务端实现者）
- 关联：**REQ-159**（契约由 join 起草）、REQ-124（控制面 API）、REQ-153（覆盖盘点）、ADR-007（构建/消费分离）、ADR-011（分工）
- 状态：**提案**。凡标「建议」的是我的判断，不是已定决策；标「现状」的是代码事实（带 `path:line`）

---

## 一 · 结论先行

| # | 结论 |
| --- | --- |
| 1 | **确认与回写不需要新开一套机制**：Console 已有的 `review/decide`、`config`、`apply`、`freeze` 就是回写通道，缺的是「把包里的待确认项汇总成一张清单」这一层 |
| 2 | 页面加**第 8 个 tab「构建包」**，内部是**三步向导 + 两个不可跳过的弹窗**（DSN、key 展示），与现有 7 个 tab 同风格（自包含单文件、壳内零数据） |
| 3 | **上传不该用 zip**：实测最大产物 300 KB，v1 用 **JSON envelope**（base64 内联）即可，省掉 `zip` 新依赖，同时把「zip 炸弹 / 路径穿越 / 解压竞态」三类风险直接消灭 |
| 4 | 真正的硬点不是页面，是 **DSN 配完之后谁用它** —— 现在源库连接是**进程级环境变量**（`src/query/serve.rs:567`），要变成**项目级**，这是架构变化，必须先拍板 |
| 5 | key 的「展示一次」要落成机制：**创建时生成、解析成功后展示、页面离开即失效（只能 rotate）**，服务端只存 `sha256` |

---

## 二 · 页面结构（第 8 个 tab，沿用现有壳）

现有壳：`GET /` 是自包含单文件、**壳内零数据**、免 token（`src/console/page.rs:185-197` 七个 `<section class="view">`，
`request()` 统一带 `Authorization`、错误分 `code/message/hint` 三段显示，`src/console/page.rs:421`）。

新增 `<section id="v-bundles">`，内容是一张**状态机驱动**的向导：

```
┌─ 构建包 ────────────────────────────────────────────────┐
│ ① 项目     [项目名] [显示名]   [建立项目并生成 key]        │  ← 已有项目则跳过
│ ② 上传     [拖入 .ontokbundle / 选择文件]                 │
│            └ 校验回执卡：manifest 摘要 · sha256 · 拒绝原因  │
│ ③ 数据库   ⚠ requires_dsn=true 时**自动弹窗**，不可跳过     │
│            └ [host][port][database][user][password][sslmode]│
│              [测试连通] → ok/latency/server_version        │
│ ④ 解析     [解析入库] → 新版本 v6 · 计数 · warnings        │
│ ⑤ 确认     待确认项清单（见 §四），逐项 ✓ 才能点「生效」    │
│ ⑥ 生效     [冻结并切流量] → ✅ 显示 key（仅此一次）         │
└──────────────────────────────────────────────────────────┘
```

**交互约束（照抄现有页面的纪律）**

| # | 约束 | 依据 |
| --- | --- | --- |
| 1 | 页面**不存 DSN**，密码框 `autocomplete="new-password"`，不写 localStorage | 与现有「口令不进 URL / 不落盘」同口径 |
| 2 | key **只在内存**，显示后不写 localStorage；刷新即失效并提示 rotate | 与「key 只显示一次」一致 |
| 3 | 每个失败必须给 `code + message + hint` 三段（现有 `errBox`） | `src/console/page.rs:467` |
| 4 | 危险动作（生效、rotate）走现有 `askConfirm` 二次确认 | `src/console/page.rs:527` |

---

## 三 · 端点映射（新增 4 个够用，其余复用）

REQ-159 §五 列了 9 个端点。按「一个端点只做一件事、能复用就不新开」重排后，

| 步 | 动作 | 端点 | 现状 |
| --- | --- | --- | --- |
| ① | 建项目 + 发 key | `POST /api/projects` | **新增**（现在是启动参数 `--project`，单项目） |
| ② | 上传包 | `POST /api/projects/:p/bundles` | **新增** |
| ② | 看包状态 | `GET /api/projects/:p/bundles/:id` | **新增** |
| ③ | 存 DSN | `PUT /api/projects/:p/database` | **新增**（加密列，永不回显） |
| ③ | 测连通 | `POST /api/projects/:p/database/test` | **新增** |
| ④ | 解析入库 | `POST /api/projects/:p/bundles/:id/import` | **新增**，内部复用 `console::ops::apply_proposal_core`（`src/console/ops.rs:143`） |
| ④ | 提案裁决 | `GET /api/review` · `POST /api/decide` | ✅ 已有（`src/console/http.rs:87-88`） |
| ⑤ | 声明回写 | `GET/PUT /api/config/:kind` | ✅ 已有（`src/console/http.rs:754`） |
| ⑥ | 生效 | `POST /api/projects/:p/bundles/:id/activate` | **新增**，内部复用 `freeze_version_core`（`src/console/ops.rs:293`） |
| —— | key 轮换 | `POST /api/projects/:p/key/rotate` | **新增** |

⇒ **真正新增 7 个端点，其中 3 个只是「包装既有函数」**（import / activate / 声明）。这与 REQ-124 的纪律一致：
> 控制面**不是一条新的写库路径**，每个端点都映射到一个既有实现（`src/console/mod.rs:8-18`）。

### 上传端点：建议 `octet-stream` 而不是 `multipart`

| 方案 | 代价 |
| --- | --- |
| `multipart/form-data` | 要开 `axum` 的 `multipart` feature（**新依赖特性**），还要处理字段名、边界、多文件 |
| **`application/octet-stream` + `?project=`** | 零新增依赖；单文件包本来就只有一个流；`fetch(body: File)` 直接发 |

⇒ 前端 `fetch('/api/projects/x/bundles', {method:'POST', body: file, headers:{'Content-Type':'application/octet-stream'}})`。

---

## 四 · 「确认与回写」到底确认什么（本设计的核心）

解析成功后，服务端返回一张**待确认清单**（不是前端拼的），每项都带 `blocking: true/false`：

| # | 确认项 | 数据从哪来 | 回写落点 | 现有？ |
| --- | --- | --- | --- | --- |
| C1 | **提案逐条裁决**（接受/拒绝/改） | `GET /api/review`（服务端从待审提案读） | `POST /api/decide` → `decisions_log` | ✅ 已有，直接复用「评审」tab 的卡片 |
| C2 | **AM 实体人签**（`reviewed_by`） | 包内 `am/access-mapping.json` 的 `entities[].reviewed_by` 为 `null` 的条目 | **新增**：签后写进快照的 mapping 部分 | ❌ 现在只能改文件（`src/access/mod.rs:338`） |
| C3 | **三份声明**（exclusions / policy / tenancy） | 包内 `declarations/*`，与当前部署文件比对 | `PUT /api/config/*`（整份替换，保存前按 schema 校验） | ✅ 已有（`src/console/decl.rs`） |
| C4 | **覆盖盘点的「不建」决策**（REQ-153 字典里的 `null`） | 包内 `coverage/gap-report.json`（若有） | **新增**：写字典 | ❌ 未实现（REQ-153 本身没开工） |

**规则**：只有「`blocking: true` 的项全部 ✓」才启用「生效」按钮。
C2 是**最容易漏**的一项：AM 里 `reviewed_by = None` 的实体，`fetch` 会**拒绝取数**（R6，`src/access/mod.rs:337`），
如果包是构建端「未签」直接打的，服务端不补签 ⇒ 上线后查询该类的客户会拿到拒绝理由，而不是数据。

---

## 五 · 实现坑（带证据，给 join 省时间）

| # | 坑 | 证据 / 修法 |
| --- | --- | --- |
| 1 | **body 上限现在只有 4 MB**，且是**全局 layer** | `src/console/http.rs:60,90`。包上限要单路由放宽：`.route("/api/projects/:p/bundles", post(..).layer(DefaultBodyLimit::max(MAX_BUNDLE_BYTES)))` —— ⛔ 不要改全局，否则 `/api/import` 也跟着放宽 |
| 2 | **不能把 100 MB 读进内存** | 边写临时文件边算 sha256；校验通过才解析。⛔ `to_bytes()` 一把梭 |
| 3 | **页面 `request()` 默认带 `Content-Type: application/json`** | `src/console/page.rs:421`。上传要**绕开**它（或给它加 `raw: true` 分支），否则服务端拿到的是 JSON envelope 而不是字节流 |
| 4 | key 现在**不是 DB 里的东西** | query token 来自启动时读一次的 `tokens.json`（`src/console/tokens.rs:21-29`「不热加载」）。项目级 key ⇒ 要么写回文件 + 重启生效，要么改成 DB 表运行期查。**这条要拍板** |
| 5 | 源库连接是**进程级 env** | `src/query/serve.rs:567` 读 `ONTOK_SOURCE_DATABASE_URL`；本体模式下 `source_pool` **恒 None**（`src/query/mod.rs:1044`）。配了 DSN 之后**谁用它**必须先定，否则 DSN 只是个「测通就丢」的摆设 |
| 6 | 快照必须是**原文**，不能是视图 | REQ-159 §十一-2 已点名（`LinkTypeView` 只有 5 个字段）。构建端我保证用 `PgStore` 直出；服务端侧**不要**在 import 前顺手做一次 view 投影 |

---

## 六 · 建议偏离 REQ-159 的一处：包容器用 JSON envelope，不用 zip

**证据**：本项目现有最大产物 300 KB（`code/out/proposal.json` 288 KB / `model-view.json` 282 KB / 主 TTL 210 KB），
REQ-159 设的 100 MB 上限在 v1 是**用不到的**。

| | zip（demand 现方案） | **JSON envelope（建议）** |
| --- | --- | --- |
| 新依赖 | `zip` crate（解压） | **0**（`serde_json` + `base64` 已在用/极易实现） |
| 路径穿越 / 符号链接 | 要靠代码逐条防 | **结构上不存在**（文件是 JSON 里的字段，不是文件系统路径） |
| zip 炸弹 | 要防（压缩比检查） | **不存在**（无解压） |
| 校验顺序 | 先解压再核 sha256 | 一遍解析即可核完 |
| 体积 | 压缩后更小 | base64 膨胀 33% ⇒ 300 KB → 400 KB，可忽略 |
| 确定性（两次打包逐字节相同） | 要钉 mtime / 顺序 / 压缩级别 | 固定字段顺序即可 |

**包长相**：

```json
{ "bundle_version": "1.0",
  "manifest": { … 与 manifest.schema.json 同形 … },
  "files": [ { "path": "ontology/snapshot.json", "sha256": "…", "bytes": 123, "b64": "…" } ] }
```

**代价**：不是标准压缩格式，用 `unzip` 打不开（但我们的用户是网页上传，不需要）。
若用户将来要「包可人工翻看」，再出 `bundle unpack` 子命令即可。

---

## 七 · 构建端交付（lion 做，REQ-159 §十一）

```bash
ontok-builder bundle pack   --project <p> --version <v> --out x.ontokbundle \
                            [--am <file>] [--db-schema <schema.json>] [--requires-dsn] \
                            [--declarations <dir>] [--built-at <rfc3339>]
ontok-builder bundle verify --file x.ontokbundle [--manifest-schema <path>]
```

| 保证 | 做法 |
| --- | --- |
| `snapshot.json` 是本体重建的唯一事实源 | `PgStore` 读出 `Snapshot` 结构体直接 `serde_json::to_vec`，**中间不过任何 view/projection** |
| 两次打包除 `built_at` 外逐字节相同 | 固定字段顺序 + 固定文件顺序 + `--built-at` 可钉 |
| 包里没有凭据 | 打包前正则扫 `password|dsn|postgres://|mysql://`；`db/schema.json` 只含表/列/类型/注释 |
| 自检 5 条（REQ-159 §十一-3） | `bundle verify` 逐条给「过/不过 + 具体文件」 |

---

## 八 · 待拍板（3 个，挡前面的是第 1 和第 2）

| # | 问题 | 选项 | 我的建议 |
| --- | --- | --- | --- |
| 1 | 用户说的「DB 都要支持」指什么 | (a) 打包**源库 schema 描述**（表/列/关系），数据不进包<br>(b) 把数据也打进包<br>(c) 构建端能从 DB 构建（DB 是输入不是产物） | **(a)**；若 (c) 也要，那是另一条（REQ-003 已覆盖） |
| 2 | **配好的 DSN 谁来用** | (a) 服务端真的连客户库查数据（⇒ 项目级连接池，架构变化）<br>(b) 只做连通性校验，证明客户环境可达（⇒ DSN 只存不用） | **(b) v1**，(a) 等「普通模式对外部署」再开（ADR-007 的 D4 触发条件） |
| 3 | key 怎么生效 | (a) 写回 `tokens.json` + 重启<br>(b) 元数据库存 key 摘要，查询服务运行期查 | **(b)**，但 v1 可先 (a) 并显式登记「需重启」 |
