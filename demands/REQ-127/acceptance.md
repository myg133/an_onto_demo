# 验收标准：REQ-127 · 查询服务提供 OWL TTL 下载

> **一句话**：Agent 能拿到整份 OWL，但**客户视图默认、建模视图显式要**，且与 CLI 导出逐字节一致。

## 一 · AC

### AC-001 · 两端点可用

```bash
GET /api/ontology.ttl              # 客户视图
GET /api/ontology.ttl?view=model   # 建模视图
```
- 都需 `Authorization: Bearer <QUERY_TOKEN>`
- 200 + `Content-Type: text/turtle; charset=utf-8`
- 判据：rdflib 能解析两个响应（0 错误）

### AC-002 · 认证

| 请求 | 期望 |
| --- | --- |
| 无 token | **401** |
| 错 token | **401** |
| **admin token** | **401**（两套凭证不通用） |
| query token | 200 |

- 判据：四种各一次

### AC-003 · 视图差异（核心安全）

客户视图里**必须没有**：

- `ontok:tenantKey` / `ontok:tenantColumn` / `ontok:tenantUnannotated`
- `ontok:joinEvidence` / `ontok:joinVerified`
- `ontok:primaryKeyColumn` / `ontok:isPrimaryKey`
- `ontok:allowedValue`

建模视图里**必须有**上述（至少 joinVerified / primaryKey / allowedValue 三类）。

- 判据：grep 计数（客户视图 0；建模视图 > 0）

### AC-004 · 与 CLI 逐字节一致

- `GET ?view=model` 的响应体 sha256 == `ontok ontology export --model-view --schema … --tenancy …` 的文件 sha256
- `GET /api/ontology.ttl` 的响应体 sha256 == `ontok ontology export`（客户视图）的文件 sha256
- 判据：两组 sha256 相等

### AC-005 · 确定性 + ETag

- 同一版本两次请求：**逐字节相同**
- 响应带 `ETag`；带 `If-None-Match: <etag>` 再请求 ⇒ **304** 且体为空
- 判据：两次 200 的 sha256 相同；304 的 `Content-Length` 为 0 或不存在

### AC-006 · 审计

- 每次请求一行：`时间 / token 摘要 / view / bytes / code`
- 判据：无 token 明文；401 也有行

### AC-007 · MCP 侧

- 二选一，**必须说明选了哪个**：
  - (a) resource：`resources/list` 返回 `ontok://ontology/{customer,model}.ttl`，`resources/read` 返回内容
  - (b) tool：`get_ontology(view, format)` 返回内容
- 判据：真调一次拿到内容，且与 HTTP 同视图的 sha256 一致

### AC-008 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` + `console` 路由 |
| 复用 | 必须复用 REQ-119 的 TTL 序列化器与 REQ-124 的导出准备逻辑 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、新增依赖 |
| 出口 | 新端点登记进 `OUTLETS`（内部轨） |
| 基线 | REQ-126 合并后的 lib 645 / doc 4 |

## 二 · 纪律

- **H** TTL 生成只许一处实现（复用，不新写）
- **G** 变异三元组：把客户视图换成建模视图 / 去掉 401 / 让 ETag 恒定 ⇒ 每条必须被抓
- **S** AC 也会错，发现直接说


---

## 三 · Dev 交付与裁定（P-1 ~ P-3）

### P-1 · AC-004 的命令口径：**按字面写死**（Dev 的选择正确）

Dev 指出：客户视图与 CLI 对拍有两种写法，**差一行文件头**，不可能同时满足。

| 写法 | 差异 |
| --- | --- |
| `ontok ontology export`（不带 `--schema`） | 客户视图**没有列名**，`--schema` 只改文件头一句「xsd 类型来源」 |
| 带 `--schema` | 多一行 `# · xsd 类型来源` |

**裁定：AC-004 以「不带 `--schema`」为准**，并把命令写死：

```bash
# 客户视图
ontok ontology export --project <P> --version <V> --out customer.ttl
# 建模视图
ontok ontology export --project <P> --version <V> --model-view --schema <S> --tenancy <T> --out model.ttl
```

**理由**：客户视图本来就不该带 schema（它不含列名），`--schema` 对它只是**文件头一句话的残留**。
⇒ **Dev 的现实现（`schema: None`）保持不变。**

⛔ QA 不要用「带 `--schema` 的客户视图」去对拍，那会红一行且毫无意义。

### P-2 · `tenantUnannotated` 在建模视图为 0：**正确，不是缺陷**

该标记是「声明缺失/未签」的**告警**（`domain/ttl.rs:1776`），与已签且命中的 `tenantKey` **互斥**。
本库 `tenancy.json` 已签且命中 ⇒ `tenantKey 54` / `tenantUnannotated 0` **正是预期**。

**要求**：QA 的 grep 判据分两类：

| 类别 | 期望 |
| --- | --- |
| `tenantKey` / `tenantColumn` / `joinEvidence` / `joinVerified` / `primaryKeyColumn` / `isPrimaryKey` / `allowedValue` | 客户视图 **0**；建模视图 **> 0** |
| `tenantUnannotated` | 两视图都允许为 0（它是告警标记，互斥项） |

### P-3 · 基线数字口径：**以「同机实测」为准**

Dev 实测 `develop@3051baf` 的 lib 基线是 **639**，而我在 AC 里写的 645（来自 REQ-126 的报告）对不上。

**原因**：部分 lib 测试依赖 `ONTOK_TEST_DATABASE_URL`（缺库时跳过/失败），**不同机器的基线数字天然不同**。

**裁定**：AC-008 的「基线 645」作废，改为：

> 基线以「同一台机器、同一环境、`develop` 干净检出」的实测为准；报告必须给出该基线数字与差值。

⇒ Dev 的 `639 → 651（+12）` 成立。
