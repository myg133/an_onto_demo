# QA 审核：REQ-136 · 本体服务模式（`query serve --ontology-only`）

> Pre-merge 审核 · P0（外部平台接入阻塞）
> 审核对象：`feature/REQ-136` @ `6e8b271`（rebase 到 `c37b21b`，1 个提交）
> 审核方式：独立构建（`CARGO_TARGET_DIR=D:\qa-req136-target\feature`）+ 真服务实测 + 独立 worktree 变异（`CARGO_TARGET_DIR=D:\qa-req136-target\mut`）
> 审核时间：2026-10-24

---

## 1 · 结论

**PASS 带非阻断（0 阻断项 / 6 条非阻断项）**

| 维度 | 结果 |
| --- | --- |
| AC-001 ~ AC-010 | **全部通过**（AC-008 按 R-3 的 200+`isError` 判） |
| catalog 真列关系 | ✅ `classes=20 · properties=47 · relations=10`，无 `excluded_relations`，无合成 `id` 列 |
| REST SPARQL | ✅ 503 `NO_DATA_SOURCE`，hint 明写「重试无效」 |
| MCP `sparql_query` | ✅ HTTP 200 + `isError=true` + `code=NO_DATA_SOURCE`；`code`/`hint`/`message` 与 REST **逐字相同** |
| 普通模式零回归 | ✅ 与同机 `develop`（`c37b21b`）**12 个端点逐字节相同**；84 类 / 952 属性 / 70 关系 / excluded 8；TTL 293,814 B；SPARQL 5 行 |
| 变异 | ✅ M1 catalog 回对账 / M2 SPARQL 200 空结果 / M3 MCP 503 / M4 污染普通模式 / M5 本体模式连源库 —— **全部被抓** |
| 零连源库 | ✅ 只连元数据库（172.17.0.87:5432）；无 `ONTOK_SOURCE_DATABASE_URL` 可起；毒 URL 不被读取 |
| 不退化 | ✅ 四红线 / `tools.rs` / `guard()` / `property_filter.rs` 零 diff；`Cargo.*` 零改动；无新增 HTTP 出口；lib 测试 +8 |

> 非阻断项见 §9。没有发现会影响外部平台接入的缺陷。

---

## 2 · catalog 真列关系（P0-1，本需求存在的理由）

### 2.1 实测（ops360 v2 冻结快照）

`GET /api/catalog?page=1&page_size=200`：

```json
{"classes":20,"properties":47,"relations":10,"skipped_classes":0}
```

- `total_items=77` = 20 类 + 47 属性 + 10 关系；
- **`counts` 里没有 `excluded_relations` 键**（只有普通模式才有）——不是 `excluded_relations=10` 那个旧形状；
- 全部属性里 `column == "id"` 的有 **0 条**；`describe` 里也没有 `id`；`primary_key` 是空串（不造假，符合 S-1）。

10 条关系（全部 `column=""`、`evidence_level="unverified"`、`join_evidence=None`）：

| # | name | from → to | evidence_level |
| --- | --- | --- | --- |
| 1 | `doc_01_6ab92914__doc_03_30a786fd` | doc_01 → doc_03 | unverified |
| 2 | `doc_01_6ab92914__doc_18_cc8cc0fd` | doc_01 → doc_18 | unverified |
| 3 | `doc_02_06730068__doc_03_30a786fd` | doc_02 → doc_03 | unverified |
| 4 | `doc_05_699ea252__doc_04_5c5a08aa` | doc_05 → doc_04 | unverified |
| 5 | `doc_06_3e3b7258__doc_09_825be34a` | doc_06 → doc_09 | unverified |
| 6 | `doc_07_afac7703__doc_01_6ab92914` | doc_07 → doc_01 | unverified |
| 7 | `doc_08_c4c943d6__doc_11_a169d3d4` | doc_08 → doc_11 | unverified |
| 8 | `doc_12_e1379a3d__doc_10_9a01297a` | doc_12 → doc_10 | unverified |
| 9 | `doc_14_0d26597c__doc_13_9ac33e15` | doc_14 → doc_13 | unverified |
| 10 | `doc_16_85c04d84__doc_12_e1379a3d` | doc_16 → doc_12 | unverified |

代表性原文：

```json
{"kind":"relation","name":"doc_01_6ab92914__doc_03_30a786fd",
 "inverse":"doc_03_30a786fd__doc_01_6ab92914",
 "from":"doc_01_6ab92914","to":"doc_03_30a786fd",
 "column":"","cardinality":"N:M",
 "evidence_level":"unverified","evidence":""}
```

### 2.2 变异 M1：让 catalog 回到「走 schema 对账」的旧行为

变异（`src/query/catalog.rs`，本体模式的关系分支）：

```rust
for (i, l) in idx.links.iter().enumerate() {
    if l.column.is_none() { continue; }   // ← MUTATION：没有 join 列就不列
    ...
}
```

| 判据 | 结果 |
| --- | --- |
| 单测 `query::catalog::tests::ontology_mode_lists_relations_that_have_no_join_column` | ❌ **红**：`left: 0, right: 1`（关系被吃掉） |
| 变异二进制 live catalog | ❌ `{"classes":20,"properties":47,"relations":0}` —— 精确复现需求原文的 bug |

✅ 变异被抓。

---

## 3 · SPARQL 语义（REST，P0-2）

`POST /api/sparql`（合法查询）实测：

```
HTTP/1.1 503 Service Unavailable
{"code":"NO_DATA_SOURCE",
 "message":"本部署只提供本体，未接业务库 —— 没有可查询的数据源，SPARQL / 取样都不可用。",
 "hint":"这个部署是**本体服务模式**（`query serve --ontology-only`）：只有本体（类 / 关系 / 属性 / TTL），
         **没有接业务库**。⛔ **重试无效**（这不是暂时故障，也不是你的查询写法问题）。…"}
```

- 状态码 **503**（不是 200、不是 500）；
- `code=NO_DATA_SOURCE`；
- hint 同时含「本体」「没有接业务库」「重试无效」；
- 错误正文不回显查询原文；审计仍记一行 `event=sparql … code=NO_DATA_SOURCE`。

### 变异 M2：改成 200 空结果

在 `post_sparql` 的本体模式里直接返回 200 空回执：

```rust
if svc.mode.is_ontology_only() {
    return respond(StatusCode::OK,
        json!({"columns":[],"rows":[],"row_count":0,"truncated":false}));
}
```

| 判据 | 结果 |
| --- | --- |
| `query::http::tests::ontology_mode_sparql_is_503_no_data_source_in_both_protocols` | ❌ **红**：`left: 200, right: 503` |

✅ 变异被抓（空结果会被调用方读成「查不到」，正是 R4 要消灭的）。

---

## 4 · MCP 语义（R-3，P0-3）

### 4.1 实测

`tools/list`：`catalog, describe, sparql_query, sample`（清单正常）。

`tools/call` → `sparql_query`：

```
HTTP 200
{"id":3,"jsonrpc":"2.0","result":{
  "content":[{"type":"text","text":"{\"code\":\"NO_DATA_SOURCE\",…}"}],
  "isError":true,
  "structuredContent":{"code":"NO_DATA_SOURCE","message":"…","hint":"…"}}}
```

逐字对拍（程序断言）：

| 字段 | REST | MCP | 相同 |
| --- | --- | --- | --- |
| `code` | `NO_DATA_SOURCE` | `NO_DATA_SOURCE` | ✅ |
| `hint` | 见 §3 | 同左 | ✅ `-ceq` True |
| `message` | 见 §3 | 同左 | ✅ `-ceq` True |
| HTTP | 503 | **200** | 按 R-3 有意不同 |

`sample` 同样是 200 + `isError=true` + `NO_DATA_SOURCE`（不返回空样例）。

### 4.2 变异 M3：MCP 也改 503

在 `mcp_post` 的 `tools/call` 分支：`isError` 时返回 `StatusCode::SERVICE_UNAVAILABLE`。

| 判据 | 结果 |
| --- | --- |
| 同一个测试 | ❌ **红**：`left: 503, right: 200`（MCP 传输层不该被判成故障） |

✅ 变异被抓。

---

## 5 · 普通模式零回归（P0-4，最容易漏）

### 5.1 实测数字（anspire_r2rml_excl v2）

| 项 | 实测 | BA 订正基线（`e72854d`） | 一致 |
| --- | --- | --- | --- |
| catalog classes | **84** | 84 | ✅ |
| catalog properties | **952** | 952 | ✅ |
| catalog relations | **70** | 70 | ✅ |
| `excluded_relations` | **8** | 8 | ✅ |
| `skipped_classes` | 0 | 0 | ✅ |
| TTL 客户视图 | **293,814 B** / sha256 `a420bbce…c070` | 293,814 B / `A420BBCE…C070` | ✅ |
| SPARQL `SELECT ?m … LIMIT 5` | **5 行** | 5 行 | ✅ |

### 5.2 与同机 develop（`c37b21b`）逐字节对拍

用同一份 schema/policy/token/source，分别起 feature 与 develop 两个二进制（18802 / 18803），12 个端点全部 **sha256 相同**：

| 端点 | 状态 | 字节 | 逐字节相同 |
| --- | --- | --- | --- |
| `/api/health` | 200/200 | 124 | ✅ |
| `/api/catalog` p1 | 200/200 | 31,671 | ✅ |
| `/api/catalog` p2 | 200/200 | 31,980 | ✅ |
| `/api/describe?name=:activity` | 200/200 | 2,207 | ✅ |
| `/api/describe?name=:activity__id` | 200/200 | 159 | ✅ |
| `/api/sparql`（5 行） | 200/200 | 1,437 | ✅ |
| `/api/sparql`（COUNT 聚合） | 200/200 | 948 | ✅ |
| `/api/ontology.ttl`（customer） | 200/200 | 293,814 | ✅ |
| `/api/ontology.ttl?view=model` | 200/200 | 371,356 | ✅ |
| MCP `tools/list` | 200/200 | 2,719 | ✅ |
| MCP `tools/call` catalog | 200/200 | 67,940 | ✅ |
| MCP `tools/call` sparql_query | 200/200 | 3,066 | ✅ |

普通模式 health **没有** `mode` 字段；catalog/describe **没有** `display_name` / `key` 新字段 —— 与基线一致。

### 5.3 变异 M4：本体模式的「不过滤」污染普通模式

在 `build_items` 开头把 `mapping` 强制成 `None`（两个模式都走快照全量分支）：

| 判据 | 结果 |
| --- | --- |
| `query::catalog::tests::query_mode_items_have_no_ontology_only_fields` | ❌ **红**（普通模式出现 `display_name`；`excluded_relations` 也没了） |

✅ 变异被抓。

---

## 6 · 模式互斥（P0-5）

| 场景 | 结果 |
| --- | --- |
| `--ontology-only` + `--schema` | **exit 1**；报错：「`--ontology-only` 与 `--schema` 互斥（R1）：本体服务模式**不做 schema 对账**……要按真实列映射：去掉 `--ontology-only`，并给 `--schema` + `--policy`」 |
| `--ontology-only --version 1`（ops360 v1 未冻结） | **exit 1**；报错：「第 1 版**尚未冻结**，拒绝对外服务……先执行 `ontok version freeze`」 |
| 普通模式缺 `--schema` | **exit 1**；报错并提示「没有源库、只想对外提供本体请加 `--ontology-only`」 |

✅ 互斥与 fail-closed 都在「绑 socket / 读 token / 连库」之前。

---

## 7 · 「该模式不许碰源库」的证据（P0-6）

### 7.1 起服务（不设源库变量、不给 schema/policy）

```powershell
Remove-Item Env:ONTOK_SOURCE_DATABASE_URL
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
ontok query serve --bind 127.0.0.1:18800 --tokens local-run/query-tokens.json `
  --project ops360 --version 2 --ontology-only
```

启动成功，横幅含：

```
· ontok query serve 就绪：绑定 127.0.0.1:18800（仅本机） · 项目 ops360 第 2 版 · …
· 模式：mode=ontology-only（不接业务库）· 无数据源 · SPARQL/取样恒 503 NO_DATA_SOURCE
· 数据源：无（本体服务模式不连业务库；SPARQL / sample 恒 503 NO_DATA_SOURCE）
```

### 7.2 进程网络连接（跑完 health/catalog/describe/SPARQL/TTL/MCP 共 18 次调用后）

```
State        LocalAddress    LocalPort  RemoteAddress   RemotePort
Established  172.17.0.1      55393      172.17.0.87     5432   ← 元数据库（快照/项目/版本）
Established  172.17.0.1      55391      172.17.0.87     5432   ← 元数据库
Listen       127.0.0.1       18800      0.0.0.0         0
```

- **没有任何**到源库 `172.16.120.20:30432` 的连接（`Get-NetTCPConnection -RemotePort 30432` 无记录）；
- 审计/日志里 `30432`、`172.16.120.20`、`SOURCE_DATABASE` 出现 **0 次**；
- 审计 18 行全部是请求事件（含 `sparql … code=NO_DATA_SOURCE`），没有连接尝试。

### 7.3 毒 URL 反证

把 `ONTOK_SOURCE_DATABASE_URL` 设成 `postgresql://nobody:bad@127.0.0.1:1/should_never_be_used` 再起本体模式：

- 服务照常启动（说明**根本不读**这个变量）；
- catalog 200 / 20·47·10，SPARQL 503；
- 进程连接仍只有元数据库；日志里 `should_never` / `127.0.0.1:1` / `nobody` **0 条**。

### 7.4 变异 M5：让本体模式仍连源库

把 `serve.rs` 的源库分支改成无条件连接，**不设** `ONTOK_SOURCE_DATABASE_URL` 起本体模式：

```
✗ 缺少环境变量 ONTOK_SOURCE_DATABASE_URL（查询服务查的是源库，不是元数据库）
exit=1
```

✅ 被 AC-001 的 live 判据抓住（基线同命令可正常启动）。

> 附带观察：只删掉 `run_sparql_event` 的第一道 503 守卫时测试**仍绿** —— 因为执行边界还有第二道守卫（`(Some(mapping), Some(source)) = … else { no_data_source() }`）。这是**双保险**，不是漏洞。

---

## 8 · 其余（P0-7）

### 8.1 describe 的 `display_name`

| 实体 | `display_name` | `key` | 属性数 | tenant.mode |
| --- | --- | --- | --- | --- |
| `:doc_15_775cd9b7`（中文名「费用概览」） | **费用概览** | `doc_15_775cd9b7` | 7 | `not_applicable` |
| `:doc_metrics`（属性最多） | **业务口径** | `doc_metrics` | 10 | `not_applicable` |
| `:doc_01_6ab92914__p_01_3a3839f6`（属性） | **资源总数** | — | column=`p_01_3a3839f6` | — |

`primary_key=""`、无 `id` 列；属性业务名非空。✅

### 8.2 TTL 与 CLI 逐字节相同（AC-005）

| 视图 | HTTP 字节 | CLI 字节 | sha256 | 相同 |
| --- | --- | --- | --- | --- |
| customer | 21,335 | 21,335 | `ab0530cf5aea7b665b6ed0c668b56fb1a035507e5e43f3b883b384367b4669d5` | ✅ |
| model | 29,687 | 29,687 | `8058c87d8eabd2d9ab87409cbf12d727c90fb044f2803782df27ac69dc3a4c4b` | ✅ |

- 与 `.local/q136.ttl` / `.local/ops360/ops360.ttl` / `.local/ops360-package/ops360.ttl` 的 sha256 **也相同**；
- `?view=model` 不依赖 schema/tenancy，明确产出并与 CLI 同参数逐字节相同（不是半成品）。

### 8.3 `/metrics`（AC-009，rebase 后集成验证）

- 免 token GET ⇒ **200**；
- 六指标齐：`ontok_requests_total`、`ontok_request_duration_seconds`、`ontok_rows_returned_total`、`ontok_tenant_denied_total`、`ontok_ttl_bytes`、`ontok_mapping_joins`；
- `ontok_ttl_bytes{view="customer"} 21335`（= ops360 TTL 字节）；
- 本体模式 `ontok_mapping_joins 0`（没有映射，是事实）；普通模式 `70`。

### 8.4 不退化

| 项 | 结果 |
| --- | --- |
| 改动文件 | 仅 8 个：`cli.rs`、`query/{catalog,error,http,mod,serve}.rs`、`tests/test_req123_live.rs`、`tests/test_req127_live.rs`（后两个只是 `ServiceParts` 新字段适配） |
| 四红线（`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs`、`src/domain/review_view.rs`） | **零 diff**；`test_req107_red_lines` 通过 |
| `src/mcp/tools.rs`（`guard()`） | **零 diff** |
| `src/domain/property_filter.rs` | **零 diff** |
| `src/domain/pipeline.rs`（`OUTLETS`） | **零 diff**；未新增 `.route(`，故无需登记；`test_req107_outlet_pipeline` 12 项通过 |
| `Cargo.toml` / `Cargo.lock` | **零 diff**（无新依赖） |
| lib 测试 | feature **708 通过 / 710 总数**；develop **700 通过 / 702 总数** ⇒ **+8**，无删除；两处失败两边完全同名（环境/DB 夹具依赖，非本分支引入） |
| `query::` 子集 | 97 通过（含 8 条 REQ-136 判据） |

---

## 9 · 阻断项 / 非阻断项

### 阻断项

**无。**

### 非阻断项

| # | 项 | 说明 / 建议 |
| --- | --- | --- |
| N-1 | 任务书里的提交号 `7d5ea73` 是 rebase **前**的；当前分支 tip 是 **`6e8b271`** | 两者只差 `tests/test_req123_live.rs` / `test_req127_live.rs` 两处适配。审核按 `6e8b271` 做，结论不受影响 |
| N-2 | 任务书 AC-006 写的「83 类 / 78 关系」是 **TTL 口径**；catalog 口径是 **84 / 70 / excluded 8** | BA 已在 `e72854d` 订正；实测与订正后一致。⛔ 报告/验收里不要再混用两个口径 |
| N-3 | 10 条关系的 `evidence_level` 是 **`unverified`**，不是字面 `false` | 这是第 4 个合法值（快照 `join_verified=None` = 这一版没记录，TTL 里不写 `ontok:joinVerified`）。语义正确，但建议在接入文档里写明 `unverified ≠ 已验证为假` |
| N-4 | catalog 用**建模视图**词汇（`doc_15_775cd9b7` / `p_01_…`）；**客户视图** TTL 用中文名（`:费用概览__信用额度`） | 这是 REQ-119/127 既有约定（普通模式同样如此），非本需求引入。两边靠 `display_name` / `rdfs:label` 对应，`?view=model` 与 catalog 同名。建议接入文档写清「catalog 对应 model 视图词汇」 |
| N-5 | `.feature/manifest.json` 仍写着 **REQ-123**（`base_commit=e3da616`），本需求未更新 | 不影响代码；合入前请 BA 按「1 worktree = 1 req」更新/归档 |
| N-6 | Dev 备注里的「lib 563」与实测口径不同（实测 710 总数 / 708 通过） | 只影响记录；本分支相对 develop **+8 且无退化**，建议以实测数为准 |

---

## 附 · 复现

```powershell
# 独立构建
$env:CARGO_TARGET_DIR='D:\qa-req136-target\feature'; cargo build --bin ontok

# 本体模式（不设源库）
Remove-Item Env:ONTOK_SOURCE_DATABASE_URL
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
ontok query serve --bind 127.0.0.1:18800 --tokens local-run/query-tokens.json `
  --project ops360 --version 2 --ontology-only

# 普通模式
$env:ONTOK_SOURCE_DATABASE_URL='postgresql://postgres:postgres123@172.16.120.20:30432/anspire_open_uat'
ontok query serve --bind 127.0.0.1:18802 --tokens local-run/query-tokens.json `
  --project anspire_r2rml_excl --version 2 `
  --schema code/out/schema.json --policy code/query-policy.json
```

原始证据：`D:\MyCodes\AI\an_onto_demo\qa-req136-artifacts\`
（`health.json` / `rest.json` / `mcp.json` / `http-*.ttl` / `cli-*.ttl` / `normal-compare.json` / `metrics.txt` / 各实例 `*.err.log` 与 `*-audit.log`）

变异验证在独立 worktree `qa-req136-mut`（detached @ `6e8b271`）+ 独立 `CARGO_TARGET_DIR`，**未改动 feature 分支，未 push**。
