# QA 审核：REQ-127 · 查询服务提供 OWL TTL 下载

> 审核角色：QA Agent（Pre-merge）
> 被测：`feature-REQ-127` / `feature/REQ-127` @ `458c577`（2 个提交，基线 `develop` = `3051baf`，merge-base = `5e73813`）
> 审核方式：**真服务 + 真 HTTP + 真库**（非 mock），另用独立 worktree 做变异
> 审核日期：2026-10-23

---

## 1 · 结论

**PASS（带 5 条非阻断观察）** — 无阻断项。

| AC | 结论 | 一句话证据 |
| --- | --- | --- |
| AC-001 两端点可用 | ✅ | 两视图 200 + `text/turtle; charset=utf-8`；rdflib 7.6.0 解析 0 错误（7146 / 9157 三元组） |
| AC-002 认证 | ✅ | 无 token / 错 token / **admin token** 均 401；query token 200（admin token 先在 console 上验明 200） |
| AC-003 视图差异 | ✅ | 客户视图 7 个建模关键词**全 0**；建模视图 54/54/156/157/84/84/104（P-2：`tenantUnannotated` 两视图 0） |
| AC-004 与 CLI 逐字节 | ✅ | 客户 `a420bbce…cc070`、建模 `2382cd9a…fd0d0`，**HTTP 与 CLI 文件同 sha256** |
| AC-005 ETag / 304 | ✅ | ETag = sha256 前 16 位；`If-None-Match` 命中 ⇒ 304、0 字节、无 `Content-Length` |
| AC-006 审计 | ✅ | 18 次请求 18 行；200/304/401 都有行；含 `view=` / `bytes=`；无 token 明文 |
| AC-007 MCP resource | ✅ | `resources/list` 两条 uri；`resources/read` 两视图 sha256 **与 HTTP 同视图相等**；未知 uri 报错带 hint |
| AC-008 不退化 / 零依赖 | ✅ | 基线同机实测 639 → 651（**+12**）；四红线 + `tools.rs` + `guard()` + `property_filter.rs` + console 路由 diff 全空；`Cargo.*` 零改动；OUTLETS 含 I25 |

---

## 2 · 提交完整性

| 项 | 结果 |
| --- | --- |
| `git rev-list --count develop..HEAD` | **2** ✅（`9d706f8` 实现 + `458c577` 真库实测） |
| `git status --short` | **空** ✅ |
| HEAD | `458c577` |
| merge-base(`develop`, HEAD) | `5e73813`（REQ-126 合并） |
| `develop` tip | `3051baf`（比 merge-base 多 1 个 **Infra** 提交：Dockerfile / .dockerignore） |
| 相对 merge-base 的改动 | 11 文件，+1863 / −25 |
| 独立 worktree `cargo check --lib` | ✅ `Finished dev profile … in 51.64s`（在 `458c577` 干净检出上跑） |

> ⚠️ **口径提示（非缺陷）**：`git diff develop..HEAD` 会显示 `Dockerfile` / `.dockerignore` 被「删除」——
> 这是分支图的假象（分支从 `5e73813` 长出，`develop` 在其后又加了 Infra 提交）。
> `git diff --stat 5e73813..3051baf` = 仅 +97 行（Dockerfile 84 + .dockerignore 13），
> 分支从未删过这两个文件，按 merge-base 开 PR 不会出现删除。**开 PR 时 base 选 `develop` 即可。**

改动清单（相对 merge-base）：

```
src/cli.rs                                M   （serve 新增 --tenancy）
src/domain/pipeline.rs                    M   （OutletId::I25 + OUTLETS）
src/query/audit.rs                        M   （AuditRecord 增 view / bytes）
src/query/http.rs                         M   （GET /api/ontology.ttl + MCP resources/list|read）
src/query/mod.rs                          M   （QueryService.ttl + audit_ttl）
src/query/serve.rs                        M   （启动时渲染两视图 + resolve_tenancy）
src/query/ttl.rs                          A   （两视图缓存 / ETag / 无序列化实现）
tests/test_req107_outlet_pipeline.rs      M   （59→60、内部 29→30）
tests/test_req107_source_enumeration.rs   M   （登记 src/query/ttl）
tests/test_req123_live.rs                 M   （夹具补 ttl 字段）
tests/test_req127_live.rs                 A   （627 行真库判据）
```

---

## 3 · 端点与 401 四种（AC-001 / AC-002）

**真进程**：`ontok query serve --bind 127.0.0.1:18080 --project anspire_r2rml_excl --version 2 --schema code/out/schema.json --policy query-policy.json`
（元数据库 `postgresql://postgres:postgres@172.17.0.87:5432/anspire_real`（podman `ontok-pg`）；源库只读 UAT；v2 已冻结 84/78。二进制 = `target/debug/ontok.exe`，`cargo build` 最新，启动横幅正常。）

| # | 请求 | 期望 | 实测 |
| --- | --- | --- | --- |
| 1 | `GET /api/ontology.ttl` 无 token | 401 | **401** ✅ `{"code":"UNAUTHORIZED",…}`（222 B，`application/json`） |
| 2 | 错 token（错 1 位） | 401 | **401** ✅ |
| 3 | **admin token**（console 那套） | 401 | **401** ✅ |
| 4 | query token | 200 + `text/turtle; charset=utf-8` | **200** ✅ `text/turtle; charset=utf-8` |

**admin token 是真凭据（关键）**：同一 token 在 `ontok console serve` 上 `GET /api/projects` ⇒ **200**；
同一把 query token 打 console 管理端点 ⇒ **403**（`TOKEN_KIND_FORBIDDEN`）。
⇒ 「两套凭证不通用」在**两个方向**都被证实，而非「随便一个串都 401」。

补充判据（都符合 R1/R5）：

| 请求 | 结果 |
| --- | --- |
| `?view=bogus` + query token | **400**（不静默回退客户视图），审计记 `code=BAD_REQUEST` |
| `?token=<query token>` 放 query string（无 header） | **401**（只认 `Authorization` 头） |
| `?view=model` 无 token | **401**，审计行 `view=model` |
| `Accept-Encoding: gzip` | 200、**无** `Content-Encoding`（未压缩，见非阻断 1） |

**AC-001 解析判据**（rdflib 7.6.0，对**响应体字节**）：

```
http_customer.ttl  OK triples= 7146
http_model.ttl     OK triples= 9157
```

---

## 4 · 两视图 grep（AC-003，核心安全）+ 变异

对**响应体**（不是本地文件；HTTP 直出的字节）计数，并与 CLI 文件交叉核对：

| 关键词 | 客户视图（HTTP） | 建模视图（HTTP） | CLI 客户 | CLI 建模 |
| --- | --- | --- | --- | --- |
| `ontok:tenantKey` | **0** | 54 | 0 | 54 |
| `ontok:tenantColumn` | **0** | 54 | 0 | 54 |
| `ontok:joinEvidence` | **0** | 156 | 0 | 156 |
| `ontok:joinVerified` | **0** | 157 | 0 | 157 |
| `ontok:primaryKeyColumn` | **0** | 84 | 0 | 84 |
| `ontok:isPrimaryKey` | **0** | 84 | 0 | 84 |
| `ontok:allowedValue` | **0** | 104 | 0 | 104 |
| `ontok:tenantUnannotated` | 0 | **0**（P-2 允许） | 0 | 0 |

补充的结构性判据（比关键词更强）：

| 项 | 客户视图 | 建模视图 |
| --- | --- | --- |
| `ontok:column`（列名三元组） | **0** | 952 |
| `ontok:tenant*` 全部 | **0** | 108 |
| `owl:NamedIndividual`（实例数据） | **0** | **0** |
| 租户标识 `C0001` | **0** | **0** |
| 内部列名 `customer_id` 明文 | **False** | **True** |

⇒ 客户视图不是「碰巧没命中」：列名三元组本身就不存在（C6 结构性不含列名），
且两视图都**不含任何客户数据 / 租户值**（R2）。

### 变异（独立 worktree `458c577`，跑完即删）

| 变异 | 改法 | 结果 |
| --- | --- | --- |
| **M1 默认视图改成建模视图**（任务要求） | `ttl_view_from_query`：`let mut view = TtlView::Customer` → `Model` | ✅ **3 条红**：`ttl_defaults_to_customer_and_model_is_explicit`（默认响应里出现 `ontok:joinVerified`）、`ttl_audit_has_view_and_bytes_for_success_and_401`、`mcp_resource_reads_the_same_ttl_bytes_as_http`。`37 passed; 3 failed` |
| M2 ETag 恒定 | `TtlBody::new`：ETag 写死 `"constant-etag"` | ✅ **2 条红**：`ttl_etag_and_if_none_match`、`bytes_are_deterministic_and_the_etag_is_the_hash_prefix` |
| M3 去掉身份门 | `bearer()` 恒定返回夹具里的合法 token | ✅ **1 条红**：`ttl_accepts_only_the_query_token`（无 token 变 200 / 期望 401） |

**变异缺口：无。** 三条（G 纪律）都被既有测试抓住。

---

## 5 · 与 CLI 逐字节（AC-004，按 **P-1 口径**）

命令口径（P-1 裁定，客户视图**不带 `--schema`**）：

```powershell
# 客户视图
ontok ontology export --project anspire_r2rml_excl --version 2 --out customer.ttl
# 建模视图
ontok ontology export --project anspire_r2rml_excl --version 2 --model-view `
  --schema D:\...\code\out\schema.json --tenancy tenancy.json --out model.ttl
```

| 视图 | CLI 文件 | HTTP 响应体 | sha256 相等 | ETag |
| --- | --- | --- | --- | --- |
| 客户 | 293,814 B | 293,814 B | ✅ `a420bbcefb04074618710ffa8b760de9ed415ce7ed58e7e479c0e35c899cc070` | `"a420bbcefb040746"` |
| 建模 | 371,356 B | 371,356 B | ✅ `2382cd9aaf22763f67a88fbc2281e8ba6684416c9e0e022e8e05f88ae27fd0d0` | `"2382cd9aaf22763f"` |

**P-1 复核**（支持该裁定，不是缺陷）：客户视图「带 `--schema`」与「不带 `--schema`」两次导出的
行数相同（8080 / 8080），**只差 1 行**，且差的正是第 15 行文件头「`xsd 类型来源`」措辞：

```
15 | # · xsd 类型来源：**启发式**（未读真实列类型，--schema <schema.json> 可消除）—— schema 0 / heuristic 843
15 | # · xsd 类型来源：真实列类型（--schema 优先；未匹配/类型认不出回退启发式）—— schema 0 / heuristic 843
```

⇒ 用「带 `--schema` 的客户视图」对拍会红一行且毫无意义，**本案按 P-1 不带 `--schema`，绿**。

---

## 6 · ETag / 304（AC-005）

| 判据 | 结果 |
| --- | --- |
| 200 的 `ETag` == 内容 sha256 前 16 位（带引号） | ✅ 两视图都成立（`ETAG_HEX_LEN = 16`） |
| `If-None-Match: <etag>` 命中 | ✅ **304**、响应体 **0 字节**、`Content-Length` 缺失（AC 允许 0 或不存在）、`ETag` 回显 |
| 弱校验 `W/"2382cd9aaf22763f"` | ✅ 也 304（按 RFC 弱比较） |
| `If-None-Match` 不命中（`"deadbeefdeadbeef"`） | ✅ 200 + 完整正文 |
| 同一版本两次 200 逐字节相同 | ✅ 同进程两次 sha256 相同 |
| **跨进程**确定性 | ✅ 重启服务（且**不给** `--tenancy`，走默认解析）后两视图 ETag/sha256 与重启前、与 CLI 完全一致 |
| `Cache-Control` | `no-cache`（每次回源复核 ETag，合理） |

---

## 7 · 审计（AC-006）

真服务 `--audit-out audit.log`，18 次请求 **18 行**，一请求一行（含 401 / 304 / 400）：

```
audit ts=…Z event=auth proto=rest token=- tenant=- q=- rows=- elapsed_ms=0 code=UNAUTHORIZED view=customer
audit ts=…Z event=ttl  proto=rest token=sha256:6b2d01a3eb27 tenant=C0001 q=- rows=- elapsed_ms=0 code=OK view=customer bytes=293814
audit ts=…Z event=ttl  proto=rest token=sha256:6b2d01a3eb27 tenant=C0001 q=- rows=- elapsed_ms=0 code=NOT_MODIFIED view=customer bytes=0
audit ts=…Z event=ttl  proto=rest token=sha256:6b2d01a3eb27 tenant=C0001 q=- rows=- elapsed_ms=0 code=OK view=model bytes=371356
audit ts=…Z event=ttl  proto=mcp  token=sha256:6b2d01a3eb27 tenant=C0001 q=- rows=- elapsed_ms=0 code=RESOURCE_NOT_FOUND view=customer
```

| 判据 | 结果 |
| --- | --- |
| 200 / 304 / 401 各一行 | ✅ 18 行 = 401×3 + 200×9 + 304×1 + RESOURCE_NOT_FOUND×2 + MCP 信封×3；304 有独立 code `NOT_MODIFIED` |
| 字段含 `view` | ✅ 15 行有 `view=`（401 行也有） |
| 字段含 `bytes` | ✅ 10 行有 `bytes=`（200 = 正文字节数；304 = `0`） |
| 无 token 明文 | ✅ 通篇不含 `QA-REQ127` 任何片段；只有 `token=sha256:6b2d01a3eb27` |
| 无查询原文 | ✅ `q=-` |
| MCP 侧 | ✅ `resources/list` 走信封级一行；`resources/read` 走 TTL 行（`view` + `bytes`） |

---

## 8 · MCP resource（AC-007，选 (a) resource）

| 判据 | 结果 |
| --- | --- |
| `initialize.capabilities.resources` | ✅ 声明（`{subscribe:false,listChanged:false}`），且 instructions 写明两个 uri |
| `resources/list` | ✅ 恰好 2 条：`ontok://ontology/customer.ttl` / `ontok://ontology/model.ttl`，`mimeType = text/turtle; charset=utf-8` |
| `resources/read` 客户 | ✅ 293,814 B，sha256 `a420bbce…cc070` == `GET /api/ontology.ttl` |
| `resources/read` 建模 | ✅ 371,356 B，sha256 `2382cd9a…fd0d0` == `GET /api/ontology.ttl?view=model` |
| 未知 uri | ✅ JSON-RPC `-32602` + `data.code=RESOURCE_NOT_FOUND` + `hint: 可用：ontok://ontology/customer.ttl / ontok://ontology/model.ttl`（不猜前缀） |
| 与 HTTP 同一份字节 | ✅ 同一 `OntologyTtl` 缓存；两视图 sha256 均相等 |

---

## 9 · 基线复现与差值（P-3）

**同机、同环境、干净检出**实测（`ONTOK_TEST_DATABASE_URL` 指向同一元数据库）：

| 检出 | 命令 | 结果 |
| --- | --- | --- |
| `develop@3051baf`（独立 worktree `code/`） | `cargo test --lib -- --list` | **639 tests, 0 benchmarks** |
| `feature/REQ-127@458c577` | `cargo test --lib -- --list` | **651 tests, 0 benchmarks** |
| 差值 | | **+12** |

交叉核对（静态计数，`src/` 去掉 `src/bin/`）：`#[test]` + `#[tokio::test]` = **639 / 651** —— 与 `--list` 逐一吻合。

**全量跑**（feature）：`test result: ok. 651 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`（0.36s）。

⇒ 与 Dev 的 `639 → 651（+12）` **完全一致**；AC-008 原文的「645」按 P-3 作废。

---

## 10 · 不退化 / 禁改面（AC-008）

| 项 | 结果 |
| --- | --- |
| 四红线 `tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs` / `src/domain/review_view.rs` | **diff 空**（对 `develop` 与对 merge-base 都空）✅ |
| `src/mcp/tools.rs`（含 `guard()` 实现，`tools.rs:693`） | **diff 空** ✅ |
| `src/domain/property_filter.rs`（G1–G4） | **diff 空** ✅ |
| `console` 路由（`src/server/*`、`src/console/*`） | **diff 空** ✅ |
| `Cargo.toml` / `Cargo.lock` | **零改动** ✅（新增依赖 = 0） |
| 删测试 / `#[ignore]` / 放宽断言 | 无新增 `#[ignore]`、无删除测试 ✅；3 个既有测试文件的改动都是**加字段 / 加登记**（outlet 59→60、内部 29→30、source-enumeration 登记 `ttl` 模块、REQ-123 夹具补 `ttl`），无一条断言被放松 |
| OUTLETS 含 I25 | ✅ `id: "I25"`、`spec_of(I25) = Internal/Text/Whole`；总数 60（内部 30）；lib 测试 `registry_is_the_declared_baseline` 通过 |
| 复用而非重写（纪律 H） | ✅ `ttl.rs` 无序列化实现：读/投影走 `cli::prepare_ttl_export`，渲染走 `console::ops::render_ttl_export` → `pipeline::render(C6/D9)`；两视图启动时各渲染一次并缓存 |
| 未挪 `src/bin/ontok-gate-preflight.rs` | ✅ 未触碰 |

---

## 11 · 阻断项 / 非阻断项

### 阻断项

**无。**

### 非阻断项（建议合并后跟进，不阻塞）

| # | 项 | 影响 | 建议 |
| --- | --- | --- | --- |
| 1 | `Accept-Encoding: gzip` 未实现（响应无 `Content-Encoding`） | 建模视图 371 KB 未压缩传输 | demand 写明「更好」而非必须；可作后续优化 |
| 2 | 400（非法 `view=`）与 unknown-resource 的审计行都记 `view=customer`（回退值） | 审计里「这一次要的是哪个视图」对这两类请求不精确 | 低优先，可加 `view=unknown` 或省略该字段 |
| 3 | `--tenancy` 变为可选：缺省时自动取 `--policy` 同目录 `tenancy.json`，取不到只在 stderr 警告 | 若部署时 policy 目录没有 tenancy.json，`?view=model` 会**静默少** `ontok:tenant*`（与 CLI 带 `--tenancy` 对不上） | 已在启动横幅明说且有单测；建议 runbook/部署清单写明「tenancy.json 必须与 query-policy.json 同目录或显式 `--tenancy`」 |
| 4 | 两视图在**启动时**渲染并常驻（约 0.9 MB） | 换版本 = 重启进程（与 token 表 / schema / 映射的既有口径一致，模块头已写明） | 无；换版本重启本来就是既有语义 |
| 5 | `git diff develop..HEAD` 会显示 Dockerfile/.dockerignore「被删除」 | 仅分支图假象（见 §2） | 开 PR 时 base = `develop`，或先 rebase 到 `3051baf` 让 diff 干净 |

### 已按 BA 裁定执行、未打回的项

| 裁定 | 本报告的落法 |
| --- | --- |
| **P-1** | AC-004 客户视图对拍**不带 `--schema`**，绿；另把「带 `--schema` 只差 1 行文件头」钉成证据，⛔ 未用它判红 |
| **P-2** | `ontok:tenantUnannotated` 两视图都允许 0，实测 0，**不判缺陷** |
| **P-3** | 基线以同机实测为准：639 → 651（+12），与 Dev 一致 |

### 复现资产（未入库）

`D:\MyCodes\AI\an_onto_demo\.review-workspace\req127\`：`probe.ps1` / `grep.ps1` / `probe3.ps1` / `probe4.ps1`、
两视图 HTTP 响应体（`http_customer.ttl` / `http_model.ttl`）、CLI 导出（`customer.ttl` / `model.ttl` /
`customer_with_schema.ttl`）、审计日志（`audit.log` / `audit2.log`）、`tokens.json` / `admin-tokens.json`。
变异 worktree 已按要求删除（`git worktree list` 只剩既有 5 个）。
