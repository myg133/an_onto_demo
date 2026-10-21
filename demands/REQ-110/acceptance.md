# 验收标准：REQ-110 · 内置 TTL 导出 + 页面查看与下载

> **代码基线**：`develop` `5737ecf`
> **参照物**：外部脚本导出的 `ai-ontology.ttl`（83 类 / 866 数据属性 / 52 对象属性 / 5954 三元组）—— 产品内置导出应能产出**等价**结果
> **本机限制**：全量 `cargo test` 在 Windows 编不过（unix-only bin，既有问题）

---

## 一 · AC

### AC-001 · TTL 合法且与模型视图一致

- `ontok ontology export --project anspire_ai --version 2 --model-view --out x.ttl` 产出可被 **rdflib** 解析
- 计数必须与 `ontology show --model-view` 对齐：`owl:Class == entity_types`、`DatatypeProperty == 属性总数`、`ObjectProperty == link_types`
- 判据：rdflib 解析 0 错误；三个计数逐项相等

### AC-002 · 客户视图导出**必须过守门器**

- 序列化 CustomerView 后，**先跑 `scan_modeling_terms`**，命中即拒绝导出（`exit≠0`，并报命中词）
- 判据：构造一份含「主键」「外键」等词的对象注释 ⇒ 导出必须失败；清掉后成功

### AC-003 · 确定性

- 同一 project + version 连续导出两次 ⇒ **逐字节相同**（`git diff --no-index` 无输出）
- 判据：属性按稳定顺序（key/列名）排序，不依赖 HashMap 迭代序

### AC-004 · HTTP 导出端点仍受门禁

| 请求 | 期望 |
| --- | --- |
| `GET /api/ontology.ttl` 无 token | **401** |
| 带正确 token | **200** + `Content-Type: text/turtle; charset=utf-8` + `Content-Disposition: attachment; filename="<project>-v<n>.ttl"` |
| 带错误 token | 401 |

- 判据：三种请求的状态码与响应头逐项断言

### AC-005 · `/` 免 token 且**不含任何本体数据**

- `GET /` 无 token ⇒ **200**，返回页面壳
- 页面壳里 grep 项目名 / 版本号 / 任何对象名 ⇒ **0 命中**
- 判据：用一个叫 `secretproj`、version 7 的项目起服务，`GET /` 的响应体里不得出现 `secretproj` 或图数据

### AC-006 · 其它 API 仍全部 gated

- `GET /api/graph` / `GET /api/node` / `GET /api/ontology.ttl` 无 token ⇒ **全部 401**
- 判据：逐个断言；⛔ 不许为了「页面能打开」把 API 一起放开

### AC-007 · 页面按钮导出的内容与 CLI **逐字节相同**

- 用页内 token 触发下载，比对下载文件与 `ontok ontology export` 的输出
- 判据：sha256 相等

### AC-008 · 有损项在文件头显式声明

- 文件头注释必须包含：`ActionType` 未导出及数量、`semantic` 降级为 `rdfs:comment`、`cardinality` 映射限制
- 判据：grep 三个关键词；⛔ 不许静默丢失

### AC-009 · 新出口必须接入 REQ-107 的判据

- `ttl.rs` 与 HTTP 路由要登记进 `OUTLETS` / 出口判据（否则 REQ-107 的「漏接即红」就漏了这一条）
- 判据：REQ-107 的 13 个判据套件全绿，且新出口出现在登记表里

### AC-010 · 改写而非删除 REQ-093 的判据

- 原判据「未登录探测不到任何路径」必须**改写**为「`/` 返回无数据页面壳，`/api/*` 一律 401」
- 判据：该测试仍存在且断言新行为；⛔ 不许删除、不许 `#[ignore]`
- 配套：新增一条 **ADR**（`BA/decisions/ADR-004-页面可达性与门禁.md`），写清取舍与代价

### AC-011 · 不退化

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、**新增依赖** |
| 基线 | lib 230 / doc 4 / 35 套件 344，一条不许少 |

---

## 二 · 纪律

- **G** 变异三元组；⛔ 编译失败不算执行
- **H** 不许第二份判据（TTL 生成与 HTTP 响应必须共用同一个函数）
- **I** 每条命令自带 `cd` 或绝对路径
- **S** AC 自己也会是错的 —— 发现不成立直接说
