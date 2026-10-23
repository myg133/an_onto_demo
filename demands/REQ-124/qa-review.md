# REQ-124 · 控制面 API —— Pre-merge QA 审核

| 项 | 值 |
| --- | --- |
| 审核对象 | `feature-REQ-124` / `881bcfd`（基线 `develop` `fbae80a`） |
| 审核时间 | 2026-10-23 |
| 审核方式 | **独立起服务 + 真 HTTP 实测**（不采信 Dev 测试结论）；在 `qa-req124-mut`（`git worktree add --detach 881bcfd`）做变异，已回收 |
| 数据库 | `postgresql://postgres:postgres@172.17.0.87:5432/anspire_real` |
| schema | `D:\MyCodes\AI\an_onto_demo\code\out\schema.json`（252,686 B） |
| 声明 | 工作树根 `feature-REQ-124\{exclusions,query-policy,tenancy}.json`；与 `code\`（develop）三份 sha256 **完全相同**。⚠️ 工作区根目录 `D:\MyCodes\AI\an_onto_demo\` 下没有这三份文件，别在 workspace 根找 |
| 被测二进制 | `feature-REQ-124\target\debug\ontok.exe`（我另起的服务端口 18894–18899，未碰 8899） |
| 分支纪律 | feature 分支零改动（`git status` 空、HEAD 仍 `881bcfd`）、未 push |

## 1 · 结论

**PASS 带非阻断**。11 条 AC 全部实测成立；无阻断项。

| AC | 判定 | 现场 |
| --- | --- | --- |
| AC-001 鉴权分离 | ✅ | 无 token 401 / query token 403 `TOKEN_KIND_FORBIDDEN` / admin 200；反向 admin → `query serve` 401 |
| AC-002 项目与版本 | ✅ | 我复跑 `test_req124_live`，计数与 `object_types` / `link_types` 的 SQL 对拍通过（口径见 §11 N5） |
| AC-003 导入 | ✅ | 内联导入 200（`p1` / 1 条 / `extracted_by=null`）；坏 JSON 400 `BAD_REQUEST`、内联缺 project 400 `PROPOSAL_MISSING_PROJECT`、路径不存在 400 `IMPORT_FILE_NOT_FOUND` |
| AC-004 应用与冻结 | ✅ | 正常 apply 200 v2；重复 apply 409 `PROPOSAL_NOT_PENDING`；重复 freeze 409 `ALREADY_FROZEN`；冻结后 import 409 `BASE_VERSION_FROZEN` |
| AC-005 TTL 逐字节 | ✅ | 客户 / 建模两组 sha256 与 CLI **相等**（§3） |
| AC-006 R2RML 逐字节 | ✅ | 默认 / `--include-inherit` 两组 sha256 与 CLI **相等**（§3） |
| AC-007 声明保存前校验 | ✅ | 9 类非法全部 400 + 点名 + **前后 sha256 相同**；合法 PUT 200 + 落盘（§4） |
| AC-008 评审端点复用 | ✅ | `GET /api/review` 三时点逐字节相同（1098 B）；decide 由同一个 `decide_json` 产出（§7） |
| AC-009 审计 | ✅ | 写/失败各一行；`/api/audit` 可数出 401×7 / 403×4；token 只有摘要；日志 grep 明文 **0 命中**（§8） |
| AC-010 绑定 | ✅ | 默认/显式回环可起；`0.0.0.0` 无 flag **exit 1 拒启**；带 flag 起 + 横幅警告（§9） |
| AC-011 不退化 | ✅ | lib 583 / doc 4 / console 14 / live 2；7 条既有红灯（口径见 §10）在 develop 逐条同因；红线/Cargo/serve/query 路由零 diff；OUTLETS 56（§10） |

## 2 · token 分离（双向）

自造真 token 表（admin + query 各一，均 ≥ 24 字符），真 HTTP 实测：

| # | 请求 | 实测 |
| --- | --- | --- |
| 1 | `GET /api/health` 无 token | **200**，正文只有 `service/version/api` |
| 2 | `GET /api/projects` 无 token | **401** `UNAUTHORIZED`（不透露路径/数据） |
| 3 | `GET /api/projects` + **query token** | **403** `TOKEN_KIND_FORBIDDEN` |
| 4 | `POST /api/import` / `/api/apply` / `/api/freeze` + query token | **403** `TOKEN_KIND_FORBIDDEN`（逐条验过） |
| 5 | `GET /api/projects` + admin token | **200**，返回项目 / 版本 / 冻结 / 计数 |
| 6 | `?token=<query>`（无 Authorization 头） | **401**（query string 不生效） |
| 7 | admin 头 + `?token=`（export） | **400**「query string 里不要放 token」 |
| 8 | **admin token → `query serve`** `/api/catalog`、`/api/describe`、`POST /api/sparql` | **401** `UNAUTHORIZED`（反向隔离成立；query token 在 query serve 200） |
| 9 | 同一 token 在 `admin-tokens.json` 出现两次（admin + query 不同 kind） | **拒绝加载，exit 1**：「第 2 条…token 与前面的重复 —— ⛔ 同一个 token 不许两用」 |
| 10 | 未知管理路径 `/api/object`（admin） | 404（不是 200；且无该路由，见 §6） |

## 3 · 与 CLI 逐字节相同（自己生成、自己比 sha256）

生产快照 `anspire_r2rml_excl` v2（解冻/只读，只读不写）；CLI 写文件，API 用 `Invoke-WebRequest -OutFile` 落文件，再 `Get-FileHash -Algorithm SHA256`：

| 产物 | 字节 | CLI sha256 | API sha256 | 相等 |
| --- | --- | --- | --- | --- |
| TTL 客户视图 | 293,829 | `fa180e85c09cf9c9599894bf7d2c8b842f8408c2486fc37e1ce504ad007a0c06` | 同左 | ✅ |
| TTL 建模视图（`--tenancy`） | 371,356 | `2382cd9aaf22763f67a88fbc2281e8ba6684416c9e0e022e8e05f88ae27fd0d0` | 同左 | ✅ |
| R2RML 默认 | 208,093 | `d6027c66f63a0a0758e7b21888b6b2fa00e7a6f2a6cc91a5ba8347cb3a843c25` | 同左 | ✅ |
| R2RML `--include-inherit` | 208,383 | `583bc92fefcc82d47e61a1e602fa57c4c9779447b3ed70970a3c5a0a553340a8` | 同左 | ✅ |

K-1 口径复核（不是按「调用点数量」判）：

- 两边消费**同一份** `ops::TtlPrepared`（`src/console/ops.rs:385`），读取/投影共用 `cli::prepare_ttl_export`（CLI `src/cli.rs:3958` ↔ HTTP `src/console/http.rs:574`）；
- 渲染都走 `domain::pipeline::render`（`C6`/`D9`）：CLI 在 `cli.rs:3980` 经 `publish_customer`，控制面在 `ops.rs:463 render_ttl_export`；
- 差异只在末端「泄漏时 exit(2) vs 返回 409」——与 K-1 裁定一致，**登记为已知偏离**，不写成「完全一致无偏离」。

## 4 · 声明校验：原文件一个字节都没变

用真 schema + 声明的副本起服务（`--exclusions/--policy/--tenancy` 指向副本），逐次比对**副本**的 sha256；同时确认仓库里的三份文件从未被写入（§4 末）。

| # | 文件 | 非法类型 | 状态 / code | 点名 | 前后 sha256 |
| --- | --- | --- | --- | --- | --- |
| 1 | policy | 表不存在 `ghost_table_qa` | **400** `DECLARATION_INVALID` | ✅ `perTable.ghost_table_qa 不在 schema 里` | 不变 |
| 2 | policy | 列不存在 `ghost_col_qa` | **400** 同上 | ✅ 点名列 + 可用列 | 不变 |
| 3 | policy | `coverage:"main_only"` 无 reason | **400** 同上 | ✅ `reason` | 不变 |
| 4 | policy | `publicDimensions` 的 reason 只有空格 | **400** 同上 | ✅ `reason` | 不变 |
| 5 | exclusions | `category:"whatever_qa"` | **400** 同上 | ✅ `category` | 不变 |
| 6 | exclusions | 缺 `decided_by` | **400** 同上 | ✅ `decided_by` | 不变 |
| 7 | exclusions | 表不存在 `ghost_table_qa` | **400** 同上 | ✅ 表名 | 不变 |
| 8 | tenancy | 列不存在 `ghost_col_qa` | **400** 同上 | ✅ 列名 | 不变 |
| 9 | tenancy | 非法 JSON `{ this is not json` | **400** 同上 | ✅ serde 行列号 | 不变 |
| + | tenancy | **合法** PUT `{"columns":["customer_id"]…}` | **200** `ok=true`，回执 sha256 | — | 落盘且 `changed=True` |

- 原三份 sha256（副本与仓库同值）：exclusions `3f77c6b8…dacc2c08`、query-policy `bca3156d…5823427c`、tenancy `71b19950…6e06b10ec5`；所有非法 PUT 前后**逐字节相同**，合法 PUT 后我立即把副本还原。
- `feature-REQ-124` 工作树 `git status --short` **空** ⇒ 仓库声明未被测试污染。
- `GET /api/config/policy` 原文返回：响应 sha256 == 文件 sha256（`bca3156d…`）。

## 5 · 冲突语义（409，code 各自不同且可读）

临时项目 `qa124conf_*` 上按真实流水线跑：

| 情形 | 状态 | code | 可读原因（摘录） |
| --- | --- | --- | --- |
| 正常 apply | 200 | — | 新版本 v2，`status=applied` |
| 重复 apply 同一提案 | **409** | `PROPOSAL_NOT_PENDING` | 「提案 p1 的状态是 applied（不是 pending）」+ hint「要改本体请新开一版」 |
| freeze 成功 | 200 | — | v2 `frozen` |
| 重复 freeze | **409** | `ALREADY_FROZEN` | 「v2 已经冻结过了（frozen_at=…），本次没有改动」 |
| 冻结后再 import（base=v2） | **409** | `BASE_VERSION_FROZEN` | 「提案基于 v2，而 v2 已冻结」 |

## 6 · 后门检查（重点）

### 6.1 路由枚举（`console` 全量，逐条判）

`src/console/http.rs:63-76` 共 11 条路由，全量如下：

| 方法 | 路径 | 落点 | 是否「直接改对象/关系/本体」 |
| --- | --- | --- | --- |
| GET | `/api/health` | 版本 | 否（免 token，零数据） |
| GET | `/api/projects` | 只读元数据库 | 否 |
| POST | `/api/import` | `cli::import_proposal_core` → `PgProposalStore::create`（只建**待审提案**） | 否（不进本体；`importable:false` 硬闸仍在） |
| POST | `/api/apply` | `ops::apply_proposal_core` → `PgApplier::apply_proposal`（既有领域函数） | 否（走「已审提案 → 新版本」既有流程） |
| POST | `/api/freeze` | `ops::freeze_for_api` → `PgProjectStore::freeze`（既有） | 否（只冻结版本） |
| GET | `/api/export/ttl` | `cli::prepare_ttl_export` + `ops::render_ttl_export` | 否（只读导出） |
| POST | `/api/r2rml` | `cli::r2rml_core` | 否（只读生成） |
| GET/PUT | `/api/config/{exclusions\|policy\|tenancy}` | 三份**声明**文件 | 否（不是本体；kind 白名单，`objects` → 404） |
| GET | `/api/audit` | 内存环 | 否 |
| GET | `/api/review` | `review_ui::http::review_json`（与 review serve 同一个） | 否 |
| POST | `/api/decide` | `review_ui::http::decide_json` → `decide_item`（既有唯一写路径） | 否（只改审核决策） |

- 全仓 `src/console` 内**没有** `PUT/PATCH/DELETE` 的自定义对象路由（唯一 `.put(` 是 `/api/config/:kind`）。
- 不存在 `PUT /api/object`、`POST /api/link`、`PATCH /api/ontology`、`POST /api/extract` 之类；请求未知路径（admin）⇒ 404。
- 结论：只暴露 import → review → apply → freeze 这条既有流程，**没有发现后门端点**。

### 6.2 写 SQL grep

```
git grep -n -E 'sqlx::query|UPDATE |INSERT |DELETE ' -- src/console/
```

命中 **1 行**，且是 `src/console/ops.rs:13` 的**文档注释**（「本模块里没有一条写 SQL」）。代码层写 SQL 命中数 = **0**（grep 工具复核同结果）。apply/freeze 调的是既有领域函数：

| 端点 | 调用链（`path:line`） |
| --- | --- |
| import | `src/console/http.rs:373` → `cli::import_proposal_core`（`src/cli.rs:3153`） |
| apply | `http.rs:432` → `ops::apply_proposal_core`（`ops.rs:143`）→ `PgApplier::apply_proposal`（`ops.rs:214`） |
| freeze | `http.rs:471` → `ops::freeze_for_api`（`ops.rs:350`）→ `PgProjectStore::freeze`（`ops.rs:331`） |
| export | `http.rs:574` → `cli::prepare_ttl_export`；`ops.rs:463` → `pipeline::render` |
| r2rml | `http.rs:653` → `cli::r2rml_core` |
| config PUT | `http.rs:742` → `decl::validate`；`:749` → `decl::save` |
| review/decide | `http.rs:848` / `:889` → `review_ui::http::{review_json, decide_json}` |

## 7 · review 端点一致性

同一项目 `qa124rev_*`、同一提案 `p1`（1 条，v1 未冻结）：

| 时点 | console `GET /api/review?proposal=p1` | review serve `GET /api/review` | 逐字节 |
| --- | --- | --- | --- |
| 决策前 | 1098 B · `36a32c6f…21da0e6` | 1098 B · `36a32c6f…21da0e6` | ✅ |
| console 签 biz 后 | 1098 B · `724a285c…20d1cc` | 同左 | ✅ |
| review serve 签 model 后 | 1098 B · `0034d61e…f0eed42` | 同左 | ✅ |

- `POST /api/decide`：console 签 `via.biz=console`、review serve 签 `via.model=ui`（K-3 允许），但**终态/进度/建议一致**——两次写入后两边 GET 仍逐字节相同，最终 `state=passed`、`progress={passed:1,signed:1,total:1}`。
- 代码层面：两个服务共用 `review_json` / `decide_json`；`src/review_ui/http.rs` 的 `.route(` 行 **0 变化**，既有 `test_req111_review_ui` 全绿。

## 8 · 审计

- 成功/失败各一行（`audit.log` 实测 27 行；`audit-conf.log` 8 行）：
  - 成功：`import`/`decide`×2/`apply`/`freeze`/`config`/`export`×2/`r2rml`×2 均 `result=OK`；
  - 失败：`apply`→`PROPOSAL_NOT_PENDING`、`freeze`→`ALREADY_FROZEN`、`import`→`BASE_VERSION_FROZEN`、`config`→`DECLARATION_INVALID`×11。
- 六字段齐全：`ts / token / action / target / result / elapsed_ms`，例：
  `audit ts=2026-10-23T08:38:22.131Z action=export token=sha256:7e3e806708e8 target=project=anspire_r2rml_excl version=2 model_view=false objects=83 relations=78 result=OK elapsed_ms=366`
- token 只记摘要（`sha256:` + 12 位）；`GET /api/audit` 正文与三个审计文件对 `QA-ADMIN-TOKEN` / `QA-QUERY-TOKEN` 明文 grep **0 命中**。
- `GET /api/audit?limit=1000` 返回 `auth` 计数：`UNAUTHORIZED`（401）**7**、`TOKEN_KIND_FORBIDDEN`（403）**4** ⇒ AC-009/K-4 的「能查到 401/403 次数」成立。
- 读端点 `GET /api/projects`、`GET /api/review` 不记（K-4 已裁定接受）。

## 9 · 绑定

| 场景 | 实测 |
| --- | --- |
| `--bind 127.0.0.1:18899` | 启动，横幅「绑定 127.0.0.1:18899（仅本机）」，无警告 |
| `--bind 0.0.0.0:18890`（无 flag） | **exit 1 拒绝启动**，文案含「默认只绑回环」「请加 `--allow-public`」 |
| `--bind 0.0.0.0:18891 --allow-public` | 启动 + 「⚠ 警告：绑定的是 0.0.0.0:18891（所有网卡）…改本体…请确认防火墙 / 反向代理 / TLS」 |
| 默认（不传 `--bind`） | clap 默认 `127.0.0.1:8899`，回环 |

## 10 · 不退化

| 项 | 实测 |
| --- | --- |
| lib | **583 passed / 0 failed**（带 `ONTOK_TEST_DATABASE_URL`） |
| doc | **4 passed** |
| 新增套件 | `test_req124_console` **14 passed**；`test_req124_live` **2 passed**（真库） |
| 7 条既有红灯 | 带 env：**5 条**（`drop_rate::real_schema_drop_rate_report`、`test_agent_orchestration::test_cli_end_to_end…`、`test_fk_naming::test_real_schema_customer_id…`、`test_retained_names_req084::test_req084_every_registered_name…`、`…no_unregistered_real_name…`）；不带 env：lib 另 **2 条**（`db::apply…write_snapshot_in_maps…`、`extract::introspect…read_only_pool…`）⇒ 共 **7 条**。**每条都在 develop `fbae80a` 上同因复现**（弹的是缺 gitignore 的脱敏 fixture / 未设 `ONTOK_TEST_DATABASE_URL`），非本需求引入。口径说明：若不设 integration env，`test_agent_orchestration::test_cli_default_and_registry_paths_remain_byte_identical` 会再多 1 条 env 红（第 8 条），它不属于本题「7 条既有红灯」口径，我也在 develop 上复现归因了 |
| 四红线 | `tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs`、`src/domain/review_view.rs` numstat **全空** |
| `tools.rs` + `guard()` | `src/mcp/tools.rs` numstat **空**（`guard()` 在 `src/mcp/tools.rs:693`，未动） |
| `property_filter.rs` | numstat **空** |
| serve / query 路由 | `src/server/http.rs`、`src/query/http.rs`、`src/query/serve.rs` numstat **全空**（§11 N1 另述 review serve 文件） |
| `Cargo.toml` / `Cargo.lock` | numstat **空**（零新依赖） |
| 无删测试 / 无 `#[ignore]` | 既有测试文件只做**增量**登记（52→56、模块表 +6），新增 2 个测试文件；`src/console` 与两个新测试文件 `#[ignore]` = 0 |
| OUTLETS | **56**（客户 17 / 建模 10 / 内部 26 / 静态 2 / 错误路径 1），I18–I21 已登记（`pipeline.rs:467-470`），断言在 `tests/test_req107_outlet_pipeline.rs:697`；我另用脚本数 `Outlet {` 也是 56 |

变异三元组（detached worktree，跑 `test_req124_console`）：

| 变异 | 结果 |
| --- | --- |
| M1 让 import 绕过 `import_proposal_core`（HTTP 直接回 200） | **被抓 2 条**：`import_goes_through_the_domain_function_so_the_importable_gate_still_fires`、`import_rejects_bad_requests_before_touching_the_database` |
| M2 config PUT 跳过 `decl::validate` | **被抓 2 条**：`six_illegal_puts_are_rejected_and_leave_the_file_byte_identical`、`audit_records_success_and_failure_without_the_token_plaintext` |
| M3 放行 query token（去掉 `kind == Admin` 判断） | **被抓 3 条**：`a_query_token_gets_403_not_401_on_management_endpoints`、`an_auth_failure_is_audited_anonymously`、`audit_records_success_and_failure_without_the_token_plaintext` |

## 11 · 阻断项 / 非阻断项

### 阻断项（0）

无。

### 非阻断项

| # | 项 | 说明 / 建议 |
| --- | --- | --- |
| N1 | `src/review_ui/http.rs` 有 345+/172- 改动（函数抽取） | K-3 要求 GET 逐字节一致，抽取 `review_json`/`decide_json` 是为此；`.route(` 0 变化、`test_req111_review_ui` 全绿、三时点逐字节相同。按 AC-011 的清单我把「serve 路由」理解为 `src/server/http.rs`（numstat 空）；若 BA 的定义包含 `review serve`，请确认这属 K-3 授权范围。**我不据此判退**。 |
| N2 | 已裁定偏离需保留在案 | K-1 两处 render 调用点（CLI `publish_customer` exit(2) / 控制面 `render_ttl_export` 返回 409）；K-3 的 `proposal.project` 必填、export 的 `schema`/`tenancy` 为服务端路径、decide 多字段与 `via=console`；K-4 读端点不审计。 |
| N3 | `GET /api/audit` 只反映**进程启动后**的内存环（≤1000 条） | 持久记录是 `--audit-out` 文件（追加、外部轮转）；建议在运维文档明说「/api/audit 是检索窗口，不是审计存储」。 |
| N4 | `admin-tokens.example.json` 的假 token 长度合法 | 原样拷贝即可通过加载（值虽写着 `REPLACE-WITH-YOUR-OWN-…`）。建议加载时对以 `REPLACE-WITH-YOUR-OWN` 开头的 token 打启动警告（防「照抄示例就上线」）。 |
| N5 | AC-002 的 SQL 对拍 | 环境里没有 `psql` 客户端；我通过复跑 `test_req124_live`（其内用 `sqlx` 对 `object_types`/`link_types` 逐项对拍）确认通过，未另写外部 SQL。属取证方式说明，不是缺陷。 |
