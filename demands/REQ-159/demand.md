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
// 请求
{ "project": "cloud-resource", "label": "腾讯云资源本体" }
// 201
{ "project": "cloud-resource",
  "key": "ontok_q_9f3c…",          // ⚠️ 只在这里出现一次，之后永不可取
  "key_digest": "sha256:3f2a1b0c9d8e",
  "created_at": "2026-10-27T10:00:00Z" }
// 409  project.exists
```

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
| 1 | 包内本体：**快照 JSON** 还是 TTL？ | **快照 JSON** | 决定包能否无损回灌 |
| 2 | 包要**签名**吗（builder 私钥 / 服务端公钥验）？ | **v1 只做 sha256 + admin 上传**，签名放 v2 | 加签名要多管一套密钥 |
| 3 | key 是**一项目一把**还是**多租户多把**？ | **v1 一项目一把**；多租户沿用 AM 的 `tenant` 字段 | 决定 `/api/projects` 的响应形状 |
| 4 | key **何时发**：创建时 / 解析成功后？ | **创建时**（用户拿到就能建连接，只是还没本体，查会 503） | 影响用户心智 |
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
