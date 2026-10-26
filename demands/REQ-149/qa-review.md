# QA 复核报告 · REQ-149 · 分页与过滤绑定的通用性（A/B/C/D）

| 项 | 值 |
| --- | --- |
| 被测 | `feature/REQ-149` @ `cb2aecc`（worktree `feature-REQ-149`） |
| 基线 | `develop` @ `0262e32`（`git archive` 独立副本 + 独立 `CARGO_TARGET_DIR`） |
| 复核者 | QA（独立契约 / 独立假端点 / 独立变异，不复用 Dev 的 `test_req149_generic.rs`） |
| 日期 | 2026-10-26 |
| 临时目录 | `feature-REQ-149/.local/qa149/`（证据 + 源码副本）· `feature-REQ-149/.local/cargo-qa149{,-base}/`（构建产物） |
| feature 分支 | **未改**（`git status` 干净、HEAD 仍 `cb2aecc`，全部动作发生在 `.local/` 副本里） |

---

## 0 · 结论

> ## **PASS（带非阻断）**

**代码层面 4 个缺口全部达标，且判据都验证过「撤销能力 ⇒ 红」**：

| 判据 | 结果 | 我的证据 |
| --- | --- | --- |
| A · 分页值类型 string | **过** | 服务端收到 `{"Limit":"100","Offset":"0"}`；变异后退回数字并红 |
| B · 页码起点 1 基（`page` / `offset_base:1`） | **过** | 第 1 页 `PageNumber=1`、第 2 页 `=2`；变异后 `PageNumber=0` 并红 |
| C · 绑定 integer + array（含**已修 AM** 的 SSL） | **过** | 收到 `"CertificateStatus":[1]`（整数），⛔ 不是 `["1"]`；变异后变字符串数组并红 |
| D · `fixture` 可选（本需求灵魂） | **过** | 四场景全对：exit 0 + 点名警告 + **无** `field_coverage` 键；`FIXTURE_REQUIRED`；真调用不受影响；`derived`/启发式都能点名 |
| 零回归（R1 + R-A 划线） | **过** | 产物：`publish` **逐字节相同**、`fetch` 仅差墙钟 `fetched_at`（基线自比对证明它是唯一变量）；`validate` 的 `errors` 与 exit code **0→0**，`warnings` 按 R-A 允许新增 |
| 无厂商特例 | **过** | `src/access/` 实现里 0 处；6 处命中全在文档/官方环境变量常量，**REQ-149 新增行 0 处** |
| 不退化 | **过** | 定向 56/9/26/7(+1 ignored)+11 全绿；全量失败集合与基线**完全一致**（除 QA 自己引入的 1 条，已修正） |

**非阻断发现（4 条，均不判 FAIL，但会影响「AM 真调用」这个上层目标）：**

| # | 发现 | 归口 |
| --- | --- | --- |
| N-1 | **AM 的 14 个 entity 全部没有 `reviewed_by`** ⇒ `fetch` 一律 `UNREVIEWED_MAPPING`；`validate` 也警告「所有 entity 都未签」 | 映射（BA 要签） |
| N-2 | **AM 没采用 A / B**：VPC 三 entity 未写 `value_type:"string"`；`AlarmPolicy` 写 `offset:"PageNumber"` 但**没有** `page`/`offset_base:1` ⇒ 第一页仍发 `PageNumber=0` | 映射（R-C：改 mapping 不改代码） |
| N-3 | `request.paging.max_limit` **存在但被静默忽略**（实际读实体级 `paging.max_limit`），且**不报错** | 映射小债 + 与 R4 精神不符（建议要么读、要么点名拒） |
| N-4 | `fixture_source:"declared"` 可以绕过反向生成启发式（作者可自称真实） | 设计已知局限（文档已写明「不核实」），非缺陷 |

---

## 1 · A / B / C 三形状（**自己造契约、自己起假端点**）

假端点：QA 手写最小 HTTP/1.1，`127.0.0.1:0`，记录**服务端收到的 body 原文**。
契约：QA 自写（A/B/C 各一份，**故意**把实体级 `paging.offset_param/limit_param` 设成别的名字，证明走的是 `request.paging`）。
测试文件：`.local/qa149/feat/tests/qa149_mine.rs`（**只在副本里**，不进 feature 分支）。

### 1.1 三条 body 原文

| 能力 | 契约要点 | 服务端**实际收到**的 body 原文 | 判定 |
| --- | --- | --- | --- |
| **A** 分页值类型 = string | `"request":{"paging":{"offset":"Offset","limit":"Limit","value_type":"string"}}`（实体级 `offset_param:"QaNotOffset"` 干扰项） | `{"Limit":"100","Offset":"0"}` | ✅ 字符串；⛔ 无 `QaNotOffset`（证明走的是 request 级路径） |
| **B-1** 页码起点（`page`） | `"paging":{"page":"PageNumber","limit":"PageSize"}`，`max_limit:2`，共 3 行 | 第 1 页 `{"PageNumber":1,"PageSize":2}`；第 2 页 `{"PageNumber":2,"PageSize":2}` | ✅ 从 1 起、页页 +1 |
| **B-2** 页码起点（`offset_base:1`） | `"paging":{"offset":"PageNumber","limit":"PageSize","offset_base":1}` | `{"PageNumber":1,"PageSize":100}` | ✅ 与 `page` 同义（BA 裁定 R-B） |
| **C** 绑定 integer+array | `"template":{"CertificateStatus":["{status}"]}` + `"bindings":{"status":{"source":"filter","value_type":"integer","shape":"array"}}`，`--filter status=1`（**单值**） | `{"CertificateStatus":[1],"Limit":100,"Offset":0}` | ✅ 整数数组；单值也包成数组 |
| **C-AM**（**已修 AM** 的 SSL） | 直接读 `D:\MyCodes\AI\an_onto_demo\.local\access-mapping\am-tencent.json`，只把 `https://ssl.tencentcloudapi.com` 换成假端点，`filter status=1` | `{"CertificateStatus":[1],"Limit":100,"Offset":0}` | ✅ 映射侧已生效 |

> **关于 key 顺序**：期望写的是 `{"Offset":"0","Limit":"100"}`，实际是 `{"Limit":"100","Offset":"0"}` —— 这是 `serde_json::Map`（BTreeMap）按字母序输出，**语义等价**，非偏差。
> **关于额外字段**：分页路径每页都会写入，所以 body 里必然带 `Limit/Offset`；判据取的是 `"CertificateStatus":[1]` 这个子串。
> **AM 未签名**：`SslCertificate` 在 AM 里没有 `reviewed_by`，真调用会被 R6 拒。C-AM 只验「body 形状」，测试内就地补了一个签名（见 N-1）。

### 1.2 三个变异（改 QA 副本源码 ⇒ 判据必须红）

变异在 `.local/qa149/feat/src/access/*` 上做（**不是** feature 分支），每次从 pristine 副本还原并 `touch` mtime，跑完恢复后已用 `git diff --no-index` 证明与 feature 工作树**逐字节相同**。

| 变异 | 撤销了什么 | 红在哪（断言原文） | 服务端收到的 body |
| --- | --- | --- | --- |
| **M-A** | `PagingValueType::String => Value::String(..)` 改成 `Value::from(n)` | `cargo exit=101`：`A 判据失败：Offset/Limit 必须是**字符串**` ／ left `{"Limit":100,"Offset":0}` vs right `{"Limit":"100","Offset":"0"}` | 数字 |
| **M-B** | `page_mode = paging_page.is_some() \|\| base == Some(1)` 改成只认 `offset_base` | `cargo exit=101`：`第 1 页必须是 1` ／ left `{"PageNumber":0,"PageSize":2}` vs right `{"PageNumber":1,"PageSize":2}` | **PageNumber=0**（正是缺口 B 的病症） |
| **M-C** | `out.push(scalar_text_json(name, ty, s)?)` 改成 `Value::String((*s).clone())` | `cargo exit=101`：`C 判据失败：必须是整数数组 [1]，收到 {"CertificateStatus":["1"],"Limit":100,"Offset":0}` | `["1"]` 字符串数组 |

> 还原后复跑：`11 passed; 0 failed`（`.local/qa149/evidence/qa149-mine-after-restore2.log`）。
> 证据：`mutants-summary.txt`、`mutant-A.log` / `mutant-B.log` / `mutant-C.log`。

---

## 2 · D · `fixture` 可选（四场景）

契约由 QA 自写（TTL 也是 QA 自写），全部走**真 CLI**（`ontok access …`）。

| 场景 | 命令 | 实测 | 判定 |
| --- | --- | --- | --- |
| **D-1** 删掉某实体的 `fixture` | `access validate`（契约里 2 条 entity：1 有夹具、1 无） | **exit 0**；stderr 有 `FIELD_PATH_NOT_CHECKED · entities[1]（id=QaNoFixture）`；headline 追加「其中 1 条**未做字段路径校准**（没有 fixture，⛔ 不是通过）」；该实体 JSON **没有** `field_coverage` 键（不是 `{}`、不是 `1/1`、也不是 `0/0`），只有 `fixture_note:"…未校准…"`；有夹具的那条仍照旧 `field_coverage` | ✅ |
| **D-2** `fetch --dry-run-fixture` 且无夹具 | `access fetch --dry-run-fixture --entity QaNoFixture` | **exit 1**；`FIXTURE_REQUIRED` + 点名 entity + 两条改法；**不返回空结果**（避免「查过了没数据」的错觉） | ✅ 明确拒绝 |
| **D-3** `fetch --allow-network` 不受影响 | ①无凭据：`--allow-network`（无夹具）→ **exit 1 `AUTH_MISSING`**（⛔ 不是 `FIXTURE_REQUIRED`）<br>②有假凭据 + QA 假端点：`--allow-network`（**无夹具**）→ **exit 0**、收到 1 次请求、`@graph` 2 个节点、`meta.network` 有回执、`freshness.source="api"` | ✅ 真调用完全不需要夹具 |
| **D-4a** 声明 `fixture_source:"derived"` | `access validate` | `FIXTURE_LOOKS_DERIVED` 警告 + 该实体 `fixture_note` 含「**没有独立证据力**」；`fixture_source:"derived"` 原样回显；exit 0（是提醒不是错误） | ✅ |
| **D-4b** 未声明来源、夹具每行**只**含映射引用字段 | `access validate` | 启发式点名：`fixture_note` 含「**疑似**按映射键反向生成」（措辞是「疑似」不是「证明」）+ 一条 `FIXTURE_LOOKS_DERIVED` 警告；此时 `field_coverage` 仍是 `1/1`（正是「自证」的样子） | ✅ |
| **D-5**（附加）写了 `fixture_source` 却没有 `fixture` | `access validate` | **exit 1**、`SCHEMA` 点名 `fixture_source`（⛔ 不静默忽略） | ✅ |

> D-3 ② 的请求体是 `{"Limit":10,"Offset":0}`，与夹具无关（`.local/qa149/evidence/net-nofixture-result.json`）。

---

## 3 · 零回归（**产物** vs **warnings** 分开报，按 R-A 精确划线）

两边二进制都是自己从源码构建的：基线 `git archive 0262e32` → `.local/qa149/base` + 独立 target；feature `git archive cb2aecc` → `.local/qa149/feat`。
**⚠️ 踩坑记录**：两个源码副本共用同一个 `CARGO_TARGET_DIR` 时，cargo 会认为「同名同版本包 = 同一个 unit」而**直接复用基线/feature 产物**（实测两次构建出的 exe sha256 完全相同）。已改为基线独立 target，并用「二进制里是否含 `FIXTURE_REQUIRED`」验证过两者确实不同。

### 3.1 产物（⛔ 一个字节都不许变）

用**旧 mapping**（`fixtures/access-cvm.json` / `fixtures/access-cvm-real.json`，都不含任何 REQ-149 新字段）：

| 产物 | 基线 `0262e32` | feature `cb2aecc` | 结论 |
| --- | --- | --- | --- |
| `publish` 的 `access.jsonld`（cvm+alarm，4370 B） | sha256 `3fd1ba56…f62c0d` | sha256 `3fd1ba56…f62c0d` | **逐字节相同** |
| `fetch` JSON-LD（cvm_instance / zone 过滤 / limit 50，50 节点） | body 见下 | body 见下 | **除墙钟 `fetched_at` 外逐字节相同**；遮掉后 sha256 `c71717ae…a75c5ad` 两边一致 |
| `fetch` JSON-LD（access-cvm-real / cvm_instance） | — | — | 遮掉 `fetched_at` 后 sha256 `dcc36aa6…590b91` 两边一致 |

**为什么可以只遮 `fetched_at`**：`fetched_at` 是 `chrono::Utc::now()`（秒级），**任何两次运行都会不同** —— 用**同一份基线二进制**连跑两次即可证明：

```
raw_equal = false        # 两次原文不同
masked_equal = true      # 只遮 fetched_at 就完全相同
fetched_at_1 = "2026-10-26T00:35:04Z"
fetched_at_2 = "2026-10-26T00:35:05Z"
raw_len_equal = true     # 长度一致（同秒时 raw 也会相等）
```

⇒ `fetched_at` 是**唯一**变量；除此之外 feature 的产物与基线**逐字节相同**。

### 3.2 `validate`（errors 与 exit 不许变；warnings 可以新增）

| 输入 | exit（基线→feature） | `errors` | `warnings` | 每条 entity 的 `field_coverage` |
| --- | --- | --- | --- | --- |
| `fixtures/access-cvm.json`（旧） | 0 → 0 | 0 → 0，**逐字相同** | 0 → 0 | **逐字相同** |
| `fixtures/access-cvm-real.json`（旧） | 0 → 0 | 0 → 0，**逐字相同** | 0 → 0 | **逐字相同** |
| `am-tencent.json`（新字段 + 合成夹具） | 0 → 0 | 0 → 0 | **1 → 14**（+13 条 `FIXTURE_LOOKS_DERIVED`） | 14 条都在，未出现「无夹具 ⇒ 无 coverage」 |

**新增的只有「提醒」**：13 条 `FIXTURE_LOOKS_DERIVED`（点名「该行只含映射字段 ⇒ 疑似反向生成，1/1 是自证」）+ 报告里每条 entity 的 `fixture_note` 字段。按 **R-A** 这是**允许多出来的诊断**，不是错误；`errors` 一条没多、`exit code` 一次没变。headline 也按要求能一眼看出「N 条未做字段路径校准 / N 条疑似反向生成」。

---

## 4 · 无厂商特例

命令：`grep -rn '(?i)tencent|vpc|monitor|billing|bill' src/access/`

| 文件:行 | 命中内容 | 性质 |
| --- | --- | --- |
| `net.rs:90` / `net.rs:92` | `pub const CANONICAL_SECRET_ID_ENV/KEY_ENV = "TENCENTCLOUD_SECRET_*"` | TC3 签名协议的**官方标准环境变量名**（通用协议细节，不是接口分支） |
| `net.rs:533` | 错误信息里列这两个环境变量名 | 同上（错误提示） |
| `net.rs:601` / `tc3.rs:86` | 文档注释里的举例 `service（cvm / monitor …）` | 文档举例 |
| `publish.rs:314` | 文档示例 JSON 里的 `credential_env` | 文档举例 |

**判断：实现里 0 处厂商/接口特例。** 另做了两条更强的检查：

1. `git diff develop...HEAD -- src/access/ | grep '^+'` 里的厂商词 = **0 行**（新增代码完全没有引入厂商名）。
2. 全 `src/access/` 搜 `action ==` / `action.eq` / `host ==` / `match ... action` / `id == "…"` = **0 处** ⇒ 没有「按接口 id / 动作名偷偷分支」，A/B/C 都是**契约字段驱动**。

> 测试/文档里提到具体产品（如 `cvm/monitor` 举例、VPC 型参数字符串）是说明性文字，**不是执行分支**；按需求「只加通用表达力」的口径，合格。

---

## 5 · 不退化

### 5.1 定向（feature，`ONTOK_REQUIRE_FIXTURES=1`）

| 目标 | 结果 |
| --- | --- |
| `cargo test --lib access::` | **56 passed / 0 failed**（756 filtered） |
| `cargo test --test test_req144_access` | **9 passed / 0 failed** |
| `cargo test --test test_req144_network` | **26 passed / 0 failed** |
| `cargo test --test test_req149_generic` | **7 passed / 0 failed / 1 ignored**（ignored = `req149_shape_evidence_with_derived_real_mapping`，需 `.local/` 真实 AM 派生副本，本次由我的 C-AM 覆盖同一能力） |
| `cargo test --test qa149_mine`（QA 自己的） | **11 passed / 0 failed** |

### 5.2 全量（`cargo test --no-fail-fast`，`ONTOK_REQUIRE_FIXTURES=1`）

> ⚠️ 规程要求三列；`ignored` 单列（它既不是 passed 也不是 SKIP）。

| | **passed** | **failed** | **SKIP** | ignored | 测试二进制 |
| --- | --- | --- | --- | --- | --- |
| **feature `cb2aecc`（我实测）** | **1910** | **35** | **3** | 3 | 109 |
| 同上，**去掉 QA 自己引入的那 1 条后（应然值）** | **1911** | **34** | **3** | 3 | 109 |
| **基线 `0262e32`（我实测，独立空库）** | **1882** | **34** | **3** | 2 | 107 |

**两边的失败集合逐条比对：feature 只多 1 条，且是 QA 自引入的**（`no_bare_env_manifest_dir_outside_anchor_file` —— 我的 `tests/qa149_mine.rs` 用了裸 `env!("CARGO_MANIFEST_DIR")`，仓库纪律禁止；已改成运行时 `current_dir()`，单跑该测试 `3 passed`）。除此之外**失败集合与基线完全相同（34 = 34，零新增）**。

**增量算术对得上**：`1882（基线）+ 11（REQ-149 新增 lib 单测 801→812）+ 7（test_req149_generic）+ 11（QA 的 qa149_mine）− 1（我的锚点测试先红后绿的中途态）= 1910`；修正后 = **1911**。

### 5.3 35 条失败逐条归因

| 类别 | 条数 | 明细 | 是不是 REQ-149 的问题 |
| --- | --- | --- | --- |
| **缺夹具 / 环境未执行**（`ONTOK_REQUIRE_FIXTURES=1` 把「响亮跳过」变成失败，`tests/common/skip.rs:118` 的**设计行为**） | **29** | `test_req123_live` 10（缺 `ONTOK_REQ123_SCHEMA`）· `test_req126_live` 6（缺 `ONTOK_REQ126_TENANT_A`）· `test_req145_live` 9（缺 `ONTOK_SOURCE_DATABASE_URL`）· `test_req124_live` 2（缺 `ONTOK_REQ124_SCHEMA`）· `test_req127_live` 1（缺 `ONTOK_REQ127_SCHEMA`）· `test_req129_doc_merge` 1（缺 `out/req116/cache/assertions.jsonl` 等本地产物） | ❌ 不是（这些判据**根本没执行**，按规程不许记 passed） |
| **基线同红**（我在基线 `0262e32` 上逐条实测复现） | **5** | `drop_rate::real_schema_drop_rate_report`（缺 `../.review-workspace/fix-props/schema.json`，全仓都没有这个私有输入）· `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name`（同上，且硬编码 Linux 路径 `/home/myg133/…`）· `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract`（基线同样 35 passed/1 failed）· `test_retained_names_req084` ×2（登记行漂移 + `src/domain/concept.rs` 9 vs 5 登记——这两个文件**不在本需求改动清单里**，基线同样红） | ❌ 不是（REQ-149 只改了 `src/access/*` + `cli.rs` + 新测试文件） |
| **QA 自己引入**（已修正） | **1** | `test_anchor_guard::no_bare_env_manifest_dir_outside_anchor_file`（我的测试文件里的裸编译期锚点） | ❌ 不是（改掉后该测试 `3 passed`，我的 11 条仍全绿） |
| **代码问题** | **0** | — | — |

> 3 条 SKIP 均为 `test_req122_live` 的「响亮跳过」（缺 `REQ122_TENANT_A/B`、`ONTOK_SOURCE_DATABASE_URL`），**不是通过**。
> 环境说明：本次共享测试库（`172.16.120.20:30432`）**全程可达**（Dev 上一轮有 27 条 DB 超时是 VPN 断线，本次没有）。基线全量我跑在**独立空库** `ontok_qa149_base_tmp` 上（`migrate()` 自动建表），跑完已 DROP，**没有二次污染共享库**。

---

## 6 · 它自报的映射问题，复核

| # | 它说 | 复核结论 | 我的证据 |
| --- | --- | --- | --- |
| **1** | `SslCertificate.local_only` 含 `status` 且模板无 `CertificateStatus` ⇒ 永远无法下推 | **属实**（修复前形态必然走本地过滤） | 把当前 AM 的 SSL 就地还原成「修复前形态」（`pushdown` 去掉 status、`local_only` 加回 status、模板删掉 `CertificateStatus`、绑定退回纯 filter）后 `--dry-run-fixture`：`pushdown_applied: []`、`filtered_locally: ["status"]`，并给出「字段 `status` 不在 `pushdown` 里 ⇒ 已在本地过滤」。而**用 BA 已修的 AM**：`pushdown_applied: ["status"]`、`filtered_locally: []`；真 shapes 也验到 `[1]`（见 §1.1 C-AM） |
| **2** | `request.paging.max_limit` 存在但解析器不读（读实体级） | **属实**，而且**静默忽略**（不报错） | 代码：`parse_request_spec` 只读 `offset/page/limit/offset_base/value_type`；`max_limit` 只在 `mod.rs:943`（实体级 `Paging`）解析，`fetch.rs:306` / `net.rs:726` 也只读实体级。行为验证：同一份 6 行夹具，`request.paging.max_limit:5` + 无实体级 ⇒ `pages=1`（按默认 100）；实体级 `paging.max_limit:5` ⇒ `pages=2` |
| **3** | 13/14 夹具被启发式点名「疑似反向生成」 | **数字对** | feature `validate` 对 AM：14 entity、0 error、14 warning = **13 条 `FIXTURE_LOOKS_DERIVED`** + 1 条「所有 entity 都未签」；13 条点名的 entity 正是除 `CvmInstance`（真实响应 `3/3`）之外的全部 13 个合成夹具 |

> **注意（R-C 归口）**：这三条都是**映射作者**的账，不是执行器的账。BA 已修 #1（SSL），#2 记小债，#3 正是 D2 想要的可见性 —— 我的复核与裁定一致。

---

## 7 · 额外非阻断发现（会挡「AM 真调用」，请 BA 决策）

### N-1 · AM 的 14 个 entity **全部未签名**

`am-tencent.json` 里 `reviewed_by` 一个都没有（14/14 空）⇒

- `validate` 给出聚合警告「所有 entity 都未签（`reviewed_by` 为空）—— 本契约可以体检，但 `fetch` 会拒绝取数（R6）」；
- 实测 `access fetch --dry-run-fixture --entity CvmInstance` ⇒ **exit 1 `UNREVIEWED_MAPPING`**。

⇒ **真调用之前必须逐条签名**，否则 BA 那边任何一条都会在第一道闸被拒（这不是 REQ-149 的缺陷，是本需求的验收前提）。

### N-2 · AM 没有采用 A / B 两个新能力（**只有 C 被采用了**）

| entity | 现状 | 后果 |
| --- | --- | --- |
| `Vpc` / `Subnet` / `SecurityGroup` | `request.paging` 只有 `offset/limit/max_limit`，**没有** `"value_type":"string"` | 仍发数字 `0/100` ⇒ 按需求描述，VPC 三接口文档要求 String，**真调用可能 `InvalidParameter`**（缺口 A 白做） |
| `AlarmPolicy` | `request.paging = {"offset":"PageNumber","limit":"PageSize","max_limit":100}` —— 用了 `offset` 语义却没有 `page`/`offset_base:1` | 第一页发 **`PageNumber=0`**（`PageNumber` 从 1 开始）⇒ 缺口 B 在 AM 上**仍然存在**（我变异 B 的实红就是 `PageNumber:0`） |
| `SslCertificate` | 已按 BA 修复：`pushdown:[filter_source,status]` + 模板 `CertificateStatus` + `integer/array` | ✅ 唯一采用了新能力的 entity |

建议（映射侧最小改动，不动代码）：VPC 三 entity 的 `request.paging` 加 `"value_type":"string"`；`AlarmPolicy` 把 `"offset":"PageNumber"` 改成 `"page":"PageNumber"`。

### N-3 · `request.paging.max_limit` 静默忽略

它既不生效、也不报错。按 **R4 的精神**（不支持的组合要报错并点名，⛔ 不许静默降级），建议后续要么真正读它（与实体级冲突时报错），要么在 `parse_request_spec` 里对 `max_limit` / `total_path` 这类实体级键**点名拒绝**。本次仅记小债（与 R-C #2 一致）。

### N-4 · `fixture_source:"declared"` 可绕过启发式

写 `"declared"` 就能让 `FIXTURE_LOOKS_DERIVED` 消失（文档已明说「工具不核实」）。这是**有意的**（工具无法证明夹具真伪，只能让人「无法在不知情的情况下」把自证当证据）。非缺陷，记录备查。

---

## 8 · 测试污染情况（如实报，**未删**）

| 时点 | `projects` 总数 | `t_*` 前缀 | 非 `t_*` |
| --- | --- | --- | --- |
| 开跑前（我实测） | **2** | **0** | 2：`cloud-resource`、`ops360` |
| 全量跑完后（我实测） | **162** | **155** | 7：`cloud-resource`、`ops360`、`cli-probe`、`req113`、`req129_ac006_…`、`req129_ac007_s_…`、`req129_ac007_u_…` |

⇒ **净增 160 个项目**，其中 **155 个是 `t_*`**，另 5 个是测试建的具名项目（`cli-probe` / `req113` / `req129_*`）。
**按指示未删除任何一条。** 我在基线全量时改用独立空库 `ontok_qa149_base_tmp`（跑完 160 个项目，已 DROP），所以共享库只被 feature 那一轮污染了一次（不是两轮）。

---

## 9 · 我的临时目录 / 清理

全部在 `feature-REQ-149/.local/`（该路径被 `.git/info/exclude` 忽略，feature 工作树 `git status` 干净）：

| 路径 | 大小 | 内容 | 处理 |
| --- | --- | --- | --- |
| `.local/qa149/evidence/` | **0.05 GB**（50.6 MB） | 判据脚本（4 个）、动态日志、三份 results JSON、两边二进制副本（各 25 MB）、变异 pristine 备份 | **保留**（本报告的证据源） |
| `.local/qa149/feat/` | 源码副本（`git archive cb2aecc`）+ QA 的 `tests/qa149_mine.rs` | 保留（可直接复跑 11 条判据） |
| `.local/qa149/base/` | 源码副本（`git archive 0262e32`） | 保留（零回归对照） |
| `.local/qa149/` 合计 | **0.07 GB**（67.3 MB） | 以上三项 | **保留** |
| `.local/cargo-qa149/` | **17.46 GB** | feature 构建产物 | 可删（重建 ~2 分钟增量 / 全量约 10 分钟） |
| `.local/cargo-qa149-base/` | **5.52 GB** | 基线构建产物 | 可删（重建约 2 分钟，依赖已缓存） |
| 元数据库 `ontok_qa149_base_tmp` | — | QA 自建基线空库 | **已 DROP**（不碰共享库里的项目） |
| `D:\` 根目录 | — | — | **没有**建任何目录 |

> 我保留了 23 GB 的两个 `CARGO_TARGET_DIR`（便于你要求「补跑/复验」时秒级出结果）；若要回收磁盘，直接删 `.local/cargo-qa149*` 两个目录即可，证据与源码副本不受影响。

---

## 附 A · 证据清单（相对 `feature-REQ-149/.local/qa149/evidence/`）

| 文件 | 内容 |
| --- | --- |
| `qa149-mine-green.log` / `qa149-mine-after-restore2.log` | QA 11 条判据全绿（含 A/B/C/C-AM 的 body 原文、D 四场景） |
| `mutants-summary.txt` + `mutant-{A,B,C}.log` | 三个变异必红的断言原文与收到的 body |
| `zero-regression.json` + `zr/*.out` `zr/*.err` `zr/*.jsonld` | 零回归逐用例结果、产物原文、publish 成品 |
| `am-checks.json` + `tmp/{feat,base}-am-validate.*` | AM 复核：13/14、未签名、SSL 下推前后、max_limit |
| `net-nofixture-result.json` | 无夹具 `--allow-network` 打假端点的回执 |
| `full-test.log` / `full-test-summary.json` | feature 全量原文 + 35 条失败分类 |
| `baseline-full-test.log` / `baseline-4-tests.log` | 基线全量（独立空库）+ 基线复现那 5 条红 |
| `bin/{feat,base}-ontok.exe` | 两边二进制（sha256 不同，已验） |
| 脚本：`mutants.ps1` / `qa149_zero_regression.py` / `qa149_am_checks.py` / `qa149_net_nofixture.py` / `parse_full_test2.py` | 全部可重放 |

## 附 B · 复现（最小）

```powershell
# 1) 建副本（不动 feature 分支）
$wt='D:\MyCodes\AI\an_onto_demo\feature-REQ-149'; $qa="$wt\.local\qa149"
git -C $wt archive --format=tar -o "$qa\feat.tar" cb2aecc
git -C $wt archive --format=tar -o "$qa\base.tar" 0262e32
& "$env:SystemRoot\System32\tar.exe" -xf "$qa\feat.tar" -C "$qa\feat"
& "$env:SystemRoot\System32\tar.exe" -xf "$qa\base.tar" -C "$qa\base"

# 2) QA 判据（A/B/C/D）
$env:CARGO_TARGET_DIR="$wt\.local\cargo-qa149"
$env:QA149_AM='D:\MyCodes\AI\an_onto_demo\.local\access-mapping\am-tencent.json'
cd "$qa\feat"; cargo test --test qa149_mine -- --nocapture

# 3) 变异（撤销 A/B/C ⇒ 必须红；跑完自动还原）
pwsh -NoProfile -File "$qa\evidence\mutants.ps1"

# 4) 零回归 + AM 复核
python "$qa\evidence\qa149_zero_regression.py"
python "$qa\evidence\qa149_am_checks.py"

# 5) 全量
$env:ONTOK_TEST_DATABASE_URL='postgresql://postgres:postgres123@172.16.120.20:30432/ontok'
$env:ONTOK_REQUIRE_FIXTURES='1'
cd "$qa\feat"; cargo test --no-fail-fast
```
