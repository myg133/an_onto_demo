# REQ-174 · **多项目各带各的 AM**：把 access mapping 随版本入库、按项目加载

- 优先级：**P1**（生产装第二套本体的前置）
- 状态：已就绪
- 触发：用户 2026-10-28「我现在要的是多项目带 AM」；生产导入 `cloud-resource` 后发现包里的 AM 被丢掉

## 一 · 现状（实测，不是推测）

| 事实 | 证据 |
| --- | --- |
| 打包**会**写 AM | `bundle pack --am` → `am/access-mapping.json`，`bundle verify` 校验它 |
| **导入完全不读 AM** | `src/query/*`、`src/db/*`、`src/console/provision.rs` 里搜不到 `AM_PATH` / `am/access-mapping` |
| 数据库里没有存 AM 的表 | `pg_tables` 无 `%access%` / `%mapping%` |
| 运行时 AM = **进程参数** | `src/query/serve.rs:723` `dep.access_raw`；`src/query/multi.rs:395` **一个进程一份** |

⇒ 后果两条：
1. 「导入包」看起来完整，**导完没有 AM**（实施人员必踩）；
2. **多项目只能共用一份 AM**（`--access-mapping` 是进程级的）。

## 二 · 要做的

### 2.1 存储：AM 挂在**版本**上（不是项目上）

```sql
CREATE TABLE IF NOT EXISTS version_access_mappings (
  version_id uuid PRIMARY KEY REFERENCES ontology_versions(id) ON DELETE CASCADE,
  raw        text        NOT NULL,   -- 包内 `am/access-mapping.json` 的**原样字节**（UTF-8）
  sha256     text        NOT NULL,   -- 与包 manifest 里的 components.am.sha256 一致
  sources    integer     NOT NULL,
  entities   integer     NOT NULL,
  stored_at  timestamptz NOT NULL DEFAULT now()
);
```

**为什么挂版本**：`activate` 换的是 `projects.active_version_id` ⇒ AM 自动跟着 active version 走，
不需要第二套"AM 生效指针"；激活/回滚语义与本体完全一致。

### 2.2 导入：有就存，**存前先验**

`POST /api/projects/:p/bundles/:id/import`：
- 包里**有** `components.am` ⇒ 读字节 → `AccessMapping::parse` → `bind(刚建好的快照模型)` → 通过才 `INSERT`；
  ⛔ 不合法 ⇒ **整笔导入失败**（版本都不建，事务回滚），错误码沿用 `am.invalid`，消息点出是包里的 AM；
- 包里**没有** ⇒ 不写行、不报错（纯库源本体不需要 AM）。

### 2.3 服务端：AM 跟着**版本**走

`serve.rs` 里构造 `QueryService` 的那一层（约 :700-740）改为**接收 AM 原文**参数，而不是直接读 `dep.access_raw`：

| 模式 | 取 AM 的顺序 |
| --- | --- |
| 单项目 | ① `--access-mapping <文件>`（兼容旧部署） ② DB 里该版本的 AM ③ 无 |
| **多项目** | ① **DB 里该 `active_version_id` 的 AM** ② `--access-mapping <文件>`（仅当 DB 无） ③ 无 |

- 缓存键仍是 `version_id`（`multi.rs` 现状）⇒ AM 与快照**同批加载**，换版本自然换 AM；
- ⛔ 不要每请求查一次 AM（除版本解析那一次外，AM 走同一批加载）。

## 三 · 验收

| # | 判据 |
| --- | --- |
| AC-001 | 导入**带** AM 的包 ⇒ `version_access_mappings` 有一行，`raw` 与包内字节**逐字节相同**、`sha256` 与 manifest 一致 |
| AC-002 | 导入**不带** AM 的包 ⇒ 无行、导入成功、`@pending` 行为与今天一致 |
| AC-003 | **多项目真进程 + 真库**：两个项目装**两份不同 AM**（不同 `base_url` 与 `service`）⇒ 各自 `@pending.get.source.base_url` / `auth.service` 是**自己那份** |
| AC-004 | 换版本：项目激活新版本（AM 不同）⇒ **下一个请求**就用新 AM（无陈旧缓存） |
| AC-005 | 单项目 + `--access-mapping 文件` ⇒ **文件优先**（旧部署字节不变，`test_req144_*` 全绿） |
| AC-006 | 包内 AM 不合法（contract 3 / 缺 `auth.service`）⇒ 导入**失败**且**没有新版本**（回滚） |
| AC-007 | v1 AM（4 键配方）照常加载（REQ-168 的 `SUPPORTED_CONTRACT_VERSIONS=(1,2)` 不回退） |
| AC-008 | 日志/审计里**不出现** AM 原文（断言 audit 行不含 `credential_env` 的值） |

**红绿自证**：把导入写 AM 的那次 `INSERT` 注掉 ⇒ AC-001 / AC-003 / AC-004 红；恢复 ⇒ 绿。

## 四 · 不在本次范围

- `PUT /api/projects/:p/access-mapping`（换 AM 不重新导入）—— 作为 P1c-3b 另立；
- AM 的**图形化编辑**（Studio）—— 不做。
