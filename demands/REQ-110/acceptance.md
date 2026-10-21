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

---

## 三 · Dev 交付后的裁定

### A-1 · AC-002 的扫描范围 = **数据正文**（头部除外）

Dev 指出 AC-002 与 AC-008 **字面互斥**：

- AC-008 要求文件头写 `cardinality`（有损声明）
- 而 `cardinality` 本身就是守门词
- ⇒ 扫整份文件的话，**每一份合法的客户导出都会被自己的文件头拒掉**

**裁定：采纳 Dev 的解法。** `TtlDocument` 把「常量头」与「正文」分开，闸只扫**正文**——
即全部**由数据产生的字节**（含 `rdfs:comment` 字面量、IRI 本地名）。

| 项 | 处理 |
| --- | --- |
| 头部常量 | 不进闸，由**独立判据**钉住（`test_req110_ttl` 断言头部内容） |
| 残余风险 | 头部是**编译期常量**，不含任何项目/对象数据；若将来头部变成动态拼接，**必须重新评估** |
| ⛔ 不许做的事 | 为了让闸通过而删掉 AC-008 的关键词 |

### A-2 · REQ-107 的 H1 golden **重锚**，接受

`/` 从「内嵌项目名的整页」变成「零数据壳」——旧 golden 固化的**正是被 R4/AC-005 推翻的行为**。
**裁定：接受重锚**，前提三条（Dev 已做到）：

1. `H1_index.html` / `SHA256SUMS` / `PROVENANCE.txt` **同步更新**，并说明重锚原因；
2. 判据仍是**逐字节**（不是放宽成「大致相同」）；
3. 其余 13 条出口判据的断言**一条不少**。

### A-3 · 客户视图 843 ≠ 建模视图 866，符合既有出口行为

差的 23 条：12 条在 `view::project` 投影时丢弃（名字不可当人话）+ 11 条被 `property_filter` 剔（源库标识符形态）。
**裁定：接受**——这与 `/api/graph`、`/api/node` 是**同一份前处理**，不是 TTL 出口的特殊行为。

### A-4 · 三元组 5953 vs 参照物 5954，差的是 `dcterms:created`

R6 要求「同版本两次导出逐字节相同」⇒ 时间戳不能进正文。
**裁定：接受**，并记录为**有意更严**：参照脚本写入了创建时间，产品导出选择确定性优先。

### A-5 · ADR-004 已落库

`BA/decisions/ADR-004-页面可达性与门禁.md`，内容取自交付报告附录 A。
