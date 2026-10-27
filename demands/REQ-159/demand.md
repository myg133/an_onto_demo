# REQ-159 · 构建包（Bundle）导入契约 · v1

- 状态：**草稿**（待拍板 6 个决策点）
- 日期：2026-10-27
- 负责：**join（服务端）** · 构建端由 **lion** 产出包
- 关联：REQ-124（控制面 API）、REQ-121（R2RML）、REQ-157（本体 v5）、ADR-012（待写）

---

## 一 · 一句话

**构建端**把「本体 + AM + 源库描述」打成**一个包**；**服务端**建项目发 key → 用户上传包 →（如有 DB）配 DSN 并测通 → 解析入库 → 生效。

---

## 二 · 角色与职责

| 角色 | 做什么 | 不做什么 |
| --- | --- | --- |
| **构建端**（lion / `ontok-builder`） | 构建本体、AM、源库 schema 描述；**打包**；包自检（sha256、manifest 完整） | ⛔ 不接触服务端凭据、⛔ 不知道 DSN |
| **服务端**（join / `ontok`） | 建项目、发 key、收包、验完整性、配 DSN 并**测连通**、解析入库、冻结/生效、出 key | ⛔ 不重新抽取、⛔ 不改包内容（只校验） |
| **用户** | 手动上传包；包内含 DB 时填 DSN | ⛔ 不需要命令行 |

---

## 三 · 包格式

**扩展名**：`.ontokbundle`（内部是 zip）

```
bundle.zip
├── manifest.json              # 必填 · 包的目录
├── ontology/snapshot.json     # 必填 · **无损快照**（不是 TTL！见 §六-1）
├── am/access-mapping.json     # 可选 · 访问映射
├── db/schema.json             # 可选 · 源库描述（表/列/关系映射，**绝不含凭据**）
├── declarations/              # 可选 · policy / tenancy / exclusions / recipe
│   ├── policy.json
│   └── tenancy.json
└── README.txt                 # 可选 · 人读
```

### `manifest.json`

```json
{
  "bundle_version": "1.0",
  "project": "cloud-resource",
  "ontology_seq": 5,
  "built_at": "2026-10-27T10:00:00Z",
  "builder": { "name": "ontok-builder", "version": "0.2.9", "host": "lion-mbp" },
  "components": {
    "ontology": { "path": "ontology/snapshot.json", "sha256": "…",
                  "entity_types": 30, "link_types": 34, "properties": 752 },
    "am":       { "path": "am/access-mapping.json", "sha256": "…", "tenants": 3 },
    "db":       { "path": "db/schema.json", "sha256": "…", "requires_dsn": true,
                  "dialect": "postgresql" }
  },
  "files": [ { "path": "ontology/snapshot.json", "sha256": "…", "bytes": 123456 } ]
}
```

**硬规则**

| # | 规则 |
| --- | --- |
| 1 | `manifest.json` 必须能解析；缺字段 ⇒ 拒（不猜默认值） |
| 2 | `files[]` 覆盖包里**每一个**文件；服务端逐个核 sha256，**任一不符即整包拒** |
| 3 | `project` 必须与**接收的项目**同名；不同名 ⇒ 拒（防串包） |
| 4 | `components.db.requires_dsn=true` ⇒ 服务端**必须**先拿到可用 DSN 才允许解析 |
| 5 | 包里**不许**出现任何凭据（密码/私钥/token）；服务端扫描到可疑字段 ⇒ 告警（v1 只告警不拒） |
| 6 | 路径不许含 `..`、绝对路径、符号链接（防穿越） |

---

## 四 · 服务端状态机

```mermaid
stateDiagram-v2
    [*] --> created: POST /api/projects（发 key，只显示一次）
    created --> uploaded: 上传包（manifest + sha256 校验通过）
    uploaded --> db_pending: 包内含 db
    uploaded --> imported: 包内无 db
    db_pending --> db_ok: 配 DSN + 测通
    db_pending --> failed: 测不通（可重试，不影响已有版本）
    db_ok --> imported: 解析入库
    imported --> active: 冻结并切流量
    imported --> failed: 解析中途失败（事务回滚）
    failed --> uploaded: 重新上传
    active --> [*]
```

**关键约束**

| # | 约束 |
| --- | --- |
| 1 | **解析永远产生新版本**，⛔ 不覆盖已冻结版本（v3 这类永久可查） |
| 2 | 解析在**单个事务**里：失败 ⇒ 一个字节都不落 |
| 3 | 切流量（`active`）是**独立一步**：解析成功 ≠ 对外生效 |
| 4 | `failed` 不阻塞已有版本继续服务 |

---

## 五 · API 契约

> 全部复用 REQ-124 控制面：**admin token** 才能调；`query` key 一律 403。
> 路径前缀 `/api`，返回 JSON，错误体 `{"error":{"code":"…","message":"…"}}`。

| # | 方法 · 路径 | 谁 | 作用 |
| --- | --- | --- | --- |
| 1 | `POST /api/projects` | admin | 建项目 + **生成 key** |
| 2 | `POST /api/projects/:p/bundles` | admin | 上传包（multipart），只校验不解析 |
| 3 | `GET /api/projects/:p/bundles/:id` | admin | 看包状态与组件清单 |
| 4 | `PUT /api/projects/:p/database` | admin | 配 DSN（加密存，**永不回显**） |
| 5 | `POST /api/projects/:p/database/test` | admin | 测连通性 |
| 6 | `POST /api/projects/:p/bundles/:id/import` | admin | **解析入库**（新版本） |
| 7 | `POST /api/projects/:p/bundles/:id/activate` | admin | 冻结并切流量 |
| 8 | `GET /api/projects/:p` | admin | 项目状态（不含 key 明文、不含 DSN） |
| 9 | `POST /api/projects/:p/key/rotate` | admin | 换 key（旧的立即失效） |

### 1 · `POST /api/projects`

```json
// 请求（admin token）
{ "project": "cloud-resource",
  "label": "腾讯云资源本体",
  "key_label": "数字员工：运维助手" }        // 给这把 key 起名，便于审计归属
// 201
{ "project": "cloud-resource",
  "key": "ontok_q_9f3c…",          // ⚠️ 只在这里出现一次，之后永不可取
  "key_kind": "query",             // ← 数字员工用的，不是 admin
  "key_digest": "sha256:3f2a1b0c9d8e",
  "bound_project": "cloud-resource",
  "created_at": "2026-10-27T10:00:00Z" }
// 409  project.exists
```

**这把 key 的定位（2026-10-27 明确）**：

| 项 | 值 |
| --- | --- |
| 谁用 | **数字员工**（agent / MCP 客户端） |
| kind | **`query`**（只能读：catalog / describe / sparql / TTL / MCP） |
| 绑定 | **一个 key ↔ 一个本体项目**（`bound_project` 写死在服务端 token 表里） |
| ⛔ 不能做 | 建项目 / 上传包 / 配 DSN / 解析 / 冻结 / 切流量（那些要 **admin** token） |

### 2 · `POST /api/projects/:p/bundles`

`multipart/form-data`，字段 `file`。

```json
// 201
{ "bundle_id": "bdl_01H…", "state": "uploaded",
  "requires_db": true,
  "components": { "ontology": {"entity_types":30,"link_types":34},
                  "am": {"tenants":3}, "db": {"dialect":"postgresql"} },
  "sha256_verified": true }
```

**校验顺序（任一失败即拒）**：体积 → zip 合法性 → 路径安全 → `manifest.json` → `files[]` sha256 → `project` 同名 → 组件计数自洽。

### 4 · `PUT /api/projects/:p/database`

```json
{ "dsn": "postgresql://user:pass@host:5432/db", "dialect": "postgresql" }
// 200
{ "database_id": "db_01H…", "host": "host", "database": "db", "user": "user" }
```
⛔ 响应里**没有** `dsn`；日志只写 host/db。

### 5 · `POST /api/projects/:p/database/test`

```json
// 200（成功）
{ "ok": true, "latency_ms": 42, "server_version": "PostgreSQL 15.4" }
// 200（失败也要 200，让 UI 能区分"测过了但不通"）
{ "ok": false, "error": { "code": "db.connect.auth", "message": "口令错误" } }
```

### 6 · `POST /api/projects/:p/bundles/:id/import`

```json
// 200
{ "version": 6, "state": "imported",
  "counts": { "entity_types": 30, "link_types": 34, "properties": 752 },
  "warnings": [] }
// 409  bundle.already_imported（幂等：同包重复导入不重复建版本）
// 422  db.dsn.missing / bundle.parse.failed
```

---

## 六 · 关键设计决定（我的立场）

### 1 · 包里放**快照 JSON**，⛔ 不放 TTL

| | 快照 JSON | TTL |
| --- | --- | --- |
| 语义 `semantic` | ✅ 完整 | ❌ **降级成注释**（代码里写明的有损项） |
| 基数 / 列名 / 证据 | ✅ 全有 | ❌ 部分 |
| 能否回灌成完整本体 | ✅ | ❌ 不可逆 |

**TTL 是给人看的出口，不是交换格式。** 用 TTL 当包内容 = 把有损格式当事实源。

### 2 · DSN 绝不进包

包是**构建产物**，会在邮件/U 盘/对象存储里流转。DSN 是**环境机密**，只能由用户在服务端当场填、当场测、加密存。

### 3 · key 只在创建时显示一次

服务端只存 `sha256(key)`。丢了就 `rotate`，⛔ 不提供"查看明文"。

### 4 · 解析放服务端 ⇒ **服务面首次获得写元数据库的能力**

这是架构变化，要写 ADR：现在的服务面基本只读（`query serve` 读冻结快照）。**必须**明确：写权限只给控制面（admin token），`query serve` 仍然只读。

---

## 七 · 待你拍板（6 个）

| # | 问题 | 我的建议 | 影响 |
| --- | --- | --- | --- |
| 1 | 包内本体：**快照 JSON** 还是 TTL？ | ✅ **已定：快照 JSON**（TTL 有损） | 决定包能否无损回灌 |
| 2 | 包要**签名**吗（builder 私钥 / 服务端公钥验）？ | **v1 只做 sha256 + admin 上传**，签名放 v2 | 加签名要多管一套密钥 |
| 3 | key 是**一项目一把**还是**多租户多把**？ | **v1 一项目一把**；多租户沿用 AM 的 `tenant` 字段 | 决定 `/api/projects` 的响应形状 |
| 4 | key **何时发**：创建时 / 解析成功后？ | **生成于创建，展示于解析成功后** —— 按你说的流程（「解析构建包，**展示 key**」）。理由：没本体的项目拿到 key 只会查 503，先展示反而是坑 | 影响用户心智与 UI 时序 |
| 5 | DSN 存哪：元数据库**加密列** / k8s Secret / 文件？ | **元数据库加密列**（AES-GCM，密钥来自环境） | 决定部署复杂度 |
| 6 | "包内含 DB" 怎么判定：`manifest.components.db` 存在 / `requires_dsn` 字段？ | **`requires_dsn=true` 才要求配 DSN**（有 `db/schema.json` 但 `requires_dsn=false` 时不弹窗） | 决定 UI 弹窗时机 |

---

## 八 · v1 验收（草案）

| # | 判据 |
| --- | --- |
| AC-001 | 建项目返回 key，且 key 只出现这一次；DB 里只有 `sha256(key)` |
| AC-002 | 上传 sha256 被篡改的包 ⇒ **整包拒**，错误码 `bundle.sha256.mismatch` |
| AC-003 | 包 `project` 与接收项目不同名 ⇒ 拒（防串包） |
| AC-004 | `requires_dsn=true` 且未配 DSN ⇒ `import` 返 **422**，且**不产生任何版本** |
| AC-005 | DSN 错误 ⇒ `database/test` 返回 `ok=false` 且**错误分类准确**（auth / timeout / dns） |
| AC-006 | 解析失败 ⇒ **零残留**（版本数不变、`ontology_versions` 无新行） |
| AC-007 | 解析成功 ⇒ 新版本，**已冻结的旧版本一个字节不变** |
| AC-008 | 任何响应 / 日志里**搜不到 DSN 明文与 key 明文**（含 500 错误栈） |
| AC-009 | `query` key 调上述任一写端点 ⇒ **403** |
| AC-010 | 同包重复 `import` ⇒ 幂等（不重复建版本，返回已有结果） |

---

## 九 · 不在 v1 范围

| 项 | 为什么 |
| --- | --- |
| 增量包 | v1 全量替换（新版本） |
| 包签名 / 供应链 | v2 |
| 服务端主动拉包（从对象存储） | 用户要求**手动上传** |
| 多项目共享一个包 | 一包一项目 |
| 自动切流量 | `activate` 单独一步，人工确认 |


---

## 十 · 需要先澄清的 3 处歧义

| # | 你说的 | 两种读法 | 影响 |
| --- | --- | --- | --- |
| 1 | 「构建端构建**本体，AM，DB** 都需要支持」 | ✅ **已定 (a)**：三者都是**构建产物**，`db/schema.json` 是**表/列/关系映射描述，不搬数据**（用户原话：我们现在抽取的 db 相关也不带数据） | 包体积与本体同量级（百 KB 级），不是 GB 级 |
| 2 | 「解析构建包，**展示 key**」 | key 是**解析成功后才给看**（我改后的建议）还是**创建时就给** | 决定 UI 时序与 AC-001 |
| 3 | 「用户**手动上传**」 | **(a)** 网页上传（服务端要出**上传页**，这是服务端 UI，归我）<br>**(b)** 用命令行/curl 调 API（用户是 FDE，能接受） | 决定 v1 要不要做页面 |

**我的假设（若你不纠正，我按这个走）**：

- 第 1 条按 **(a)**：包里有 `db/schema.json`（表/列/关系映射），**数据不进包**
- 第 2 条按**解析成功后展示**
- 第 3 条按 **(a) 网页上传** —— 因为你说"手动上传"，而 FDE 用命令行也叫手动，两者差别很大；**这条我最不确定，请明确**


---

## 十一 · 给构建端（lion）的实现清单

> 这一节是**给构建端照着做的**，不是备忘。凡是我会拒绝的，都列在 §十一-4。

### 1 · 你要交付的命令

```bash
ontok-builder bundle pack --project <p> --version <v> --out <file>.ontokbundle
```

包内四件（可选部分按需）：

| 包内路径 | 从哪来 | 硬要求 |
| --- | --- | --- |
| `manifest.json` | 你自己生成 | 符合 `BA/demands/REQ-159/manifest.schema.json` |
| `ontology/snapshot.json` | **`PgStore` 读出的 `Snapshot` 原文**（`serde_json` 直出） | 见下面 ⚠️ |
| `am/access-mapping.json` | 现在的 AM 文件原样 | 不含凭据 |
| `db/schema.json` | 表/列/关系映射 | ⛔ **不含 DSN / 密码 / 数据行** |

### 2 · ⚠️ `snapshot.json` 必须是 **`Snapshot` 原文**，⛔ 不能用 `ontology show` 的输出

**这条最容易踩**：`ontology show`（默认 JSON）和 `--model-view` 打的是**视图**，不是快照。
实测 `LinkTypeView` 只有 **5 个字段**：

```rust
pub struct LinkTypeView { key, from_type, to_type, cardinality, semantic }   // 就这 5 个
```

**没有 `evidence`、没有 `table`/`column`、没有 `joinVerified`。**
拿视图 JSON 当包内容 ⇒ **服务端入库后关系的证据全丢**，而且**丢得无声无息**。

**要求**：`ontology/snapshot.json` = `domain::snapshot::Snapshot` 的 `serde_json` 序列化**原文**，
即从 `PgStore` 直接读结构体再 `to_vec`，**中途不要经过任何 view / filter / projection**。

> 包的 `snapshot.json` 里 `version_id` / `seq` 是**构建端的标识**；
> 服务端入库时会**重新分配**版本号，不用你操心。

### 3 · 打包前自检（你跑，5 条，建议做成 `bundle verify`）

| # | 检查 | 判据 |
| --- | --- | --- |
| 1 | manifest 能按 `manifest.schema.json` 校验通过 | 用任意 JSON Schema 校验器 |
| 2 | `files[]` **覆盖包内每一个文件**（漏一个都不行） | 集合相等 |
| 3 | `files[].sha256` 与真实文件一致 | 逐文件算 |
| 4 | `snapshot.json` 里 `link_types[*].evidence` **存在**（哪怕是空数组） | 抽 1 条断言 |
| 5 | 包里**搜不到** `password` / `dsn` / `postgres://` / `mysql://` | 正则扫全文 |

### 4 · 我会拒绝的 8 种情况（提前避开）

| # | 情况 | 错误码 | 我能给你的提示 |
| --- | --- | --- | --- |
| 1 | manifest 缺字段 / 类型不符 | `bundle.manifest.invalid` | 报**具体字段路径** |
| 2 | `files[]` 与包内实际文件不一致 | `bundle.files.mismatch` | 列出**多/少/不符**的文件名 |
| 3 | 任一文件 sha256 不符 | `bundle.sha256.mismatch` | 点名文件 |
| 4 | `project` 与接收项目不同名 | `bundle.project.mismatch` | 两边名字都回显 |
| 5 | `bundle_version` 不认识 | `bundle.version.unsupported` | 我支持的版本列表 |
| 6 | 路径含 `..` / 绝对路径 / 符号链接 | `bundle.path.illegal` | 点名路径 |
| 7 | `requires_dsn=true` 但没配 DSN 就 import | `db.dsn.missing` | 提示先配 DSN |
| 8 | 包体积超上限（默认 100 MB） | `bundle.too_large` | 报实际大小与上限 |

### 5 · 联调顺序（别一次全上）

| 步 | 你给什么 | 我验什么 |
| --- | --- | --- |
| 1 | **只带 ontology** 的包（`cloud-resource` v5） | 能上传、能解析、新版本 == 现有 v5 的语义 |
| 2 | 加 `am/` | AM 能加载、租户数对得上 |
| 3 | 加 `db/schema.json` + `requires_dsn=true` | 我这边能弹 DSN、测连通、再解析 |

**每一步跑通再进下一步。** 第 1 步就是最好的兼容性测试：**用 v5 打出来的包，服务端解析回库，
应该得到与现有 v5 逐字节等价的快照**（这条我会当 AC 验）。


---

## 十二 · 一套部署 · 多套本体（**本次设计真正的价值**）

### 1 · 现状：启动即绑死

```rust
// src/query/serve.rs:386-407
let p = projects.get_by_name(&args.project).await?;              // ← 启动时定死项目
let v = projects.version_by_seq(p.id, &p.name, args.version)…;   // ← 启动时定死版本
if v.frozen_at.is_none() { bail!(…) }
let snap = store.get_snapshot(v.id).await?;                      // ← 只加载**一份**快照
```

⇒ **一个进程 = 一个项目 + 一个版本**。多一套本体就得再部署一次（现在 UAT 的 `ONTOK_VERSION=5` 就是这么来的）。

### 2 · 目标

```
一个 ontok 进程
  ├── 项目 A（cloud-resource） → 当前生效版本 v5
  ├── 项目 B（xxx）            → 当前生效版本 v2
  └── 项目 C（yyy）            → 当前生效版本 v1
```

请求进来时按 **key → 项目 → 当前生效版本** 解析；⛔ 不再依赖 `--project/--version`。

### 3 · 要改的四件

| # | 改什么 | 现状 | 目标 |
| --- | --- | --- | --- |
| 1 | **快照解析时机** | 启动时一次 | **首次请求时懒加载**（或启动预热），`activate` 后失效重载 |
| 2 | **`QueryService`** | 单实例持有单 `model` | `ProjectRegistry: Map<项目, Arc<QueryService>>` |
| 3 | **token 表** | `{token, kind, label, tenant}` | 加 **`project`** 字段：**query key 必填**（数字员工用），**admin key 必须为空**（admin 是运维的，不绑项目） |
| 4 | **生效版本指针** | `--version` 参数 | 项目表里的 `active_version`，`activate` 端点写它 |

### 4 · 兼容（**不回退已有部署**）

| 场景 | 行为 |
| --- | --- |
| 启动时给了 `--project` + `--version` | 走**单项目模式**（与今天完全一致，不动） |
| 启动时**没给** `--project` | 走**多项目模式**：按 key 解析 |

⇒ 老部署一个字节不用改；新部署用新模式。

### 5 · 风险（必须先说）

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | `QueryService` 从单例变注册表，**几乎所有读路径的入参都要改** | 先做"启动时把**所有项目**的生效版本都加载进 registry"，读路径只把 `svc` 换成 `svc_for(project)` —— **改动集中在一处**，后续再优化成懒加载 |
| 2 | 内存：N 套本体常驻 | 本体是元数据（现在 v5 的 TTL 才 210 KB），**N=100 也就 20 MB**，可接受；真大了再做 LRU |
| 3 | 匿名 Studio 无法判断项目 | Studio 的 `/api/*` 必须带 key；⛔ 匿名只能看"被允许的项目"（v1：匿名只看默认项目） |
| 4 | 缓存键从「进程级」变「项目级」 | 现有缓存若按单例假设写死，要逐个核（这是**最容易漏**的地方） |

---

## 十三 · Studio 的「＋」建项目（UI 契约）

### 1 · 位置

项目名右侧（截图红框处）：

```
☁ 元方本体平台   TURING · ONTOLOGY PLATFORM     cloud-resource 第 5 版 · 已冻结  [＋]
```

### 2 · 点开之后

```
┌─ 新建项目 ────────────────────────────┐
│ 项目名  [my-ontology            ]      │
│ 备注    [可选                    ]      │
│                                        │
│           [取消]   [创建并生成 key]     │
└────────────────────────────────────────┘
```

创建成功 → **弹一次性 key**（可复制、明确的"只显示这一次"警示）→ 直接进入**上传包**步骤。

### 3 · 安全规则（硬）

| # | 规则 |
| --- | --- |
| 1 | ⛔ **加号只在带 admin key 的会话里显示** —— 建项目是写操作，匿名/query key 一律看不到 |
| 2 | 创建请求走 `POST /api/projects`，**admin token** 鉴权 |
| 3 | key 明文只在创建响应里出现一次，前端**不落 localStorage**（只放 `sessionStorage` 且可一键清） |
| 4 | 页面刷新后 key 不再显示（服务端只存 `sha256`） |

### 4 · 页面状态

| 状态 | 加号 | 说明 |
| --- | --- | --- |
| 匿名浏览 | **隐藏** | 只读 |
| admin 已登录 | 显示 | 可建项目 |
| 项目刚创建、未上传包 | 显示 + 顶部提示"尚未导入本体" | 引导上传 |


---

## 十四 · 数字员工怎么接入（**key 的消费方**）

### 1 · 一把 key = 一个项目

```
ontok_q_9f3c…  ──►  cloud-resource（当前生效版本 v5）
ontok_q_7a1b…  ──►  my-ontology（当前生效版本 v2）
```

数字员工**不需要知道项目名、也不需要知道版本号** —— 服务端从 key 反查：

```
Authorization: Bearer ontok_q_9f3c…
        ↓ 查 token 表
   kind=query, project=cloud-resource
        ↓ 查项目当前生效版本
   v5
        ↓ 读该版本快照
   返回结果
```

**这正是「一套部署多套本体」的落地点**：换项目 = 换 key，**不用重新部署**。

### 2 · 数字员工的配置形态

```json
{
  "base_url": "https://an-notol.mic-scfw.cloudns.org",
  "api_key": "ontok_q_9f3c…",
  "project": "cloud-resource"        // 仅用于人读；服务端不以它为准
}
```

MCP 客户端同理：`Authorization: Bearer <key>`。

### 3 · 它能调什么

| 端点 | 用途 |
| --- | --- |
| `GET /api/catalog` | 有哪些类/关系（含 `semantic_label`） |
| `GET /api/describe/:name` | 某个类的字段与关系 |
| `POST /api/sparql` | 查询 |
| `GET /api/ontology.ttl` | 拿本体 |
| `GET /mcp`（SSE） | MCP 工具调用 |

⛔ 上面这些以外的写端点，一律 **403**（`key.not_admin`）。

### 4 · ⚠️ 一个必须现在就说清的隐患：**审计归属**

v1 是「**一项目一把 key**」。如果同一个项目有 **3 个数字员工**，它们**共用这一把 key**：

| 后果 | 说明 |
| --- | --- |
| **审计分不清是谁在查** | 审计日志里只有 `sha256:…`，三个员工的记录长得一模一样 |
| **丢了一把 = 全员重配** | `rotate` 会把三个员工一起打断 |
| **租户隔离靠参数** | 若靠 `tenant` 查询参数区分，那是**客户端可伪造的**，不是隔离 |

**两个选项**：

| 选项 | 做法 | 代价 |
| --- | --- | --- |
| **A（我建议）** | v1 仍**一项目一把**，但 `key_label` **必填**（写清"给谁用"），并在契约里记为**已知限制**；v2 支持「一项目多把 key，每把一把员工」 | v1 审计粒度粗 |
| B | v1 就支持一项目多把 key（每把带 `label` + 可选 `tenant`） | token 表与 UI 稍微复杂一点，但**审计、轮换、隔离一次到位** |

> 我倾向 **A**：先把链路跑通；但如果你现在就有多个数字员工要接同一个项目，那 **B 更省事**——它只是"一个项目下多行 token"，成本并不高。

### 5 · 与现有 `query-tokens.json` 的关系

现在是**一份文件装所有 key**：

```json
{"version": 1, "tokens": [{"token": "…", "kind": "query", "tenant": "…"}]}
```

加了 `project` 之后：

```json
{"version": 2, "tokens": [
  {"token": "…", "kind": "query", "project": "cloud-resource", "label": "运维助手"},
  {"token": "…", "kind": "query", "project": "my-ontology",    "label": "成本助手"},
  {"token": "…", "kind": "admin", "label": "fde-michael"}     // ← admin 无 project
]}
```

⇒ `version` 要**升到 2**（加字段是 schema 变化，按既有纪律：不升版本等于假装没变）。


---

## 十五 · 决策已定（2026-10-27 10:07）

| # | 决策 | 结论 | 影响 |
| --- | --- | --- | --- |
| 1 | 包签名 | **v1 不做**，只做 sha256 完整性 | 上线只需 admin token |
| 2 | key 与项目 | **一项目一把**（`key_label` 记用途，解决审计归属的可读性） | token 表一项目一行 |
| 3 | DSN 存储 | **元数据库加密列** | 部署无需额外 Secret |
| 4 | 生效方式 | **解析后人工激活** | `import` 与 `activate` 两个端点分开 |
| 5 | 安全强度 | **不过度设计**（见下"保留 vs 放弃"） | 减少 v1 工作量 |

### 5 · 保留 / 放弃（"先不考虑过多的安全性"的具体落点）

| 保留（成本几乎为零，且删了会出事） | 放弃（v1 不做） |
| --- | --- |
| **admin / query 二分** —— 数字员工不能建项目 | 包签名、公钥基础设施 |
| **DSN 不回显**（响应与日志都不出现明文） | DSN 的 KMS/HSM 托管、密钥轮换 |
| **key 只在创建时返回一次**（DB 只存 sha256） | key 的查看/回收站、细粒度 scope |
| **zip 路径穿越防护**（防 `../`） | 速率限制、WAF、防重放 |
| **解析单事务**（失败零残留） | 多副本一致性、跨机房 |

> 一句话：**能挡住"误操作"和"串包"的保留，防"恶意攻击"的推后。**

---

## 十六 · v1 三刀（实施拆分）

| 刀 | 内容 | 归属 | 依赖 |
| --- | --- | --- | --- |
| **P1a 后端** | 表结构（`project_keys` / `project_bundles` / `project_databases` + `projects.label` / `active_version_id`）+ 7 个端点 + 包校验（8 个错误码）+ 解析入库（单事务）+ 人工激活 | join 派 Dev | 无 |
| **P1b 多项目服务** | `query serve` 多项目模式：按 key 反查 project → active_version → 懒加载 `ProjectRegistry`；保留单项目模式兼容 | join 派 Dev | P1a |
| **P1c 前端** | Studio 项目名旁 **＋** → 建项目弹窗 → 一次性 key → 上传包向导（含 DSN 弹窗与连通性结果） | join 派 Dev | P1a |

**为什么 P1b 与 P1a 分开**：P1a 全部是**新增**（新表、新端点），不碰现有读路径；P1b 要动 `QueryService` 的单例假设，风险高一个量级。**先让包能进库，再让多项目跑起来。**
