# QA 审核报告：REQ-110 · 内置 TTL 导出 + 页面查看与下载

> **审核对象**：`feature/REQ-110` @ **`638c0dc`**（基线 `develop` `5737ecf`，1 个 Dev 提交）
> **审核方式**：独立 worktree `qa-REQ-110`（分支 `qa/REQ-110`，**未 push、未改 feature 分支**）+ 真库 `anspire_ai` v2 + 真进程 CLI/HTTP + **rdflib 7.6.0** 独立解析 + **4 组变异**
> **环境**：`postgresql://postgres:postgres@172.17.0.87:5432/ontok_demo` · Python 3.12 / rdflib 7.6.0 · cargo 1.96.0
> **审核日期**：2026-10-21
> **依据**：`acceptance.md`（11 AC + §三 A-1~A-5 裁定）、`design-summary.md`、`ADR-004`、`.feature/verification-report.md`

---

## 0 · 结论

> ## **FAIL —— 1 项阻断（B-1）**
>
> 功能面（11 条 AC）**全部通过**，头部泄漏测试**不成立**（见 §1），闸在真库、真进程、真 HTTP 上都挡得住，CLI/HTTP 逐字节相同，H1 重锚合规。
>
> **但提交 `638c0dc` 把 `src/bin/ontok-gate-preflight.rs`（2758 行，`develop` 上是跟踪文件）整份删掉了**，
> 工作区里只剩一个**未跟踪**的同内容副本（sha256 与 Dev 报告一致）。
> ⇒ **fresh checkout 的 `feature/REQ-110` 编不过** `tests/test_retained_names_req084.rs`（`include_str!` 找不到文件），
> Dev 本地全绿只是因为那个未跟踪文件还在。这是「交付物被误删 + 分支不可构建」，必须修（一条 `git add` 即可）。

### 速览

| # | 项 | 结果 |
| --- | --- | --- |
| P0-1 | **TTL 头部是否泄漏数据** | ✅ **不泄漏**（头部无「只在头部出现」的数据字节；详见 §1）<br>⚠️ 但 A-1 的措辞「头部是编译期常量，不含任何项目数据」**不成立** → 非阻断 N-1 |
| P0-2 | AC-001 计数 / rdflib / 843 vs 866 | ✅ 计数全对、rdflib 0 错误、与参照物同构<br>⚠️ 23 条的分解**不是** 12+11，实测 **11+11+1** → 非阻断 N-2 |
| P0-3 | AC-002 闸（按 A-1 口径扫正文） | ✅ 正文命中 ⇒ CLI `exit=2` / HTTP `500`，**0 字节**产物；对照组成功 |
| P0-4 | AC-005 零数据壳（自查） | ✅ `/` 200 且与 golden **逐字节相同**，项目名/版本/对象名/`@prefix`/计数 **0 命中** |
| P0-5 | AC-007 CLI 与 HTTP 逐字节 | ✅ sha256 相同；三个响应头逐项断言通过；出口层直调序列化器 **0 处** |
| P0-6 | AC-003 / AC-008 | ✅ 两次导出逐字节相同；三类有损声明 + 数量在头部 |
| P0-7 | AC-010 判据改写合规 | ✅ **改写而非删除**；`/api/*` 断言一条不少；H1 重锚有 PROVENANCE 说明；其余 12 份 golden 字节未动；无放宽 |
| P0-8 | 不退化 | ✅ lib 245 / doc 4 / 32 套件 313 / 新增 14；红线与 `Cargo.*` 零 diff<br>❌ **B-1：preflight 文件被删** |
| P0-9 | 变异（G） | ✅ 4 组变异全部被现有判据抓住（§3） |
| — | **结论** | **FAIL（1 阻断 + 8 非阻断）** |

---

## 1 · 头部泄漏测试（P0-1，单独成节）

### 1.1 要回答的问题

TTL 文件由两段组成（`ttl.rs::finish`）：

```text
【文件头】@prefix 8 行  +  `#` 有损声明 4 行      ← 不进闸（A-1）
【正 文】<本体节点> + 类块 + 属性块 + 关系块      ← 进闸（scan_modeling_terms）
```

任务书怀疑：头部有 `ontok:sourceProject "<项目名>"` 这类由数据产生的字面量，而 A-1 只扫正文 ⇒ 新泄漏通道。

### 1.2 实测：项目名含建模术语（`外键登记表`）

夹具：`secretproj` 式真库项目 `外键登记表_45900`（v1，2 对象 / 1 关系 / 3 个 ActionType），**真进程**导出：

```text
project="外键登记表_45900" exit=Some(2) stdout=0 字节
stderr=⚠ 客户视图泄露了建模术语（出口 C6）：["外键"]
--out 现场：exit=Some(2)  文件存在=false  文件大小=None
```

| 问题 | 答案 |
| --- | --- |
| 项目名进了 TTL 吗？ | 会进**正文**：`ontok:sourceProject "外键登记表_45900"`、`rdfs:label "… 本体（v1）"@zh` —— 但**文件根本没被写出来**（stdout 0 字节、`--out` 目标不存在） |
| 被拒了吗？ | ✅ **被拒**：`exit=2`（`EXIT_LEAKED_MODELING_TERMS`），命中词条 `["外键"]` |
| 结论 | 中文术语项目名**不会**泄漏：它在正文里，闸看得见 |

> 补充：`--out` 指向**已存在**的文件时，失败导出**不写**（旧内容原样保留，不截断）——见 N-5。

### 1.3 头部还有哪些**动态内容**（逐个判定）

| # | 头部字节 | 是否数据派生 | 能否携带敏感值 | 判定 |
| --- | --- | --- | --- | --- |
| 1 | `@prefix : <{base}{local_model(project)}#> .` | **是**（`local_model` 把非 `[0-9A-Za-z_]` 换成 `_`） | 能携带**项目名的 ASCII 归一化形态** | ⚠️ 但**同一字符串在正文里逐字出现**（正文本体节点 `<{base}{local_model(project)}> a owl:Ontology`）⇒ 闸在正文上已覆盖同一值，**无正文外信息** |
| 2 | `# · ActionType：未导出（{action_type_count} 个）` | **是**（计数） | 只泄露「动作层有几条」（数字），不含内容；AC-008 强制要求 | 可接受（实测 4 个 ActionType ⇒ 头里写「未导出（4 个）」） |
| 3 | 其余 7 行 `@prefix` | 否（编译期常量） | 否 | 安全 |
| 4 | 其余 4 行有损声明（含守门词 `cardinality` / 记号 `1:1`） | 否（常量） | 否 | 安全（这正是 A-1 排除它的原因） |

**关键验证**：构造一个「项目名归一化后**恰好等于**守门词」的样例 `extracted-by`
（`local_model("extracted-by") = "extracted_by"`，而 `extracted_by` 是 `MODELING_TERMS` 成员）：

```text
project="extracted-by" exit=Some(2) stdout=0 字节
stderr: ⚠ 客户视图泄露了建模术语（出口 C6）：["extracted_by"]
```

⇒ **被拒**。原因：`extracted_by` 不只出现在头部的 `@prefix` 里，**也出现在正文的本体 IRI** 里
（`<https://ontok.local/extracted_by> a owl:Ontology`，`ontology_header()` 属于 `body`）。
所以「头部有一份、正文没有」的形态**构造不出来**。

对照（同形态但正文 token 不同）：项目 `extracted-by_45900` ⇒ 导出成功，头部与正文都是
`extracted_by_45900`（token ≠ `extracted_by`），`scan(body)` 为空 —— 行为正确。

### 1.4 本节结论

1. **没有新泄漏通道。** 头部所有动态字节（项目名归一化形态、动作层计数）**都不携带正文之外的信息**：
   第 1 项与正文本体 IRI 逐字相同，第 2 项是计数。
2. **A-1 的措辞不准确**：原文「头部是**编译期常量**，不含任何项目/对象数据」与代码不符
   （头部第 1 行含 `local_model(项目名)`，第 2 行含 `action_type_count`）。
   裁定**结论**（只扫正文）仍成立，但**理由**要改成：
   「头部动态部分的每一个字节都与正文中的同一字节重复；排除头部不减少任何覆盖」。
3. **建议（零成本加固，不违反 AC-008）**：把闸的扫描范围从 `body` 扩到
   `prefix_block + body`（即「除有损声明块外的全部字节」）——
   有损声明块里才有 `cardinality`/`1:1` 这两个必挂的常量；前缀块与正文扫起来无副作用。
   或退一步：在 `test_req110_ttl` 里把头部钉成「常量模板 + `local_model(project)` + 计数」，
   让「头部将来变成动态拼接」时当场红。
4. 附带确认：模型视图（`--model-view`）**不**扫（`Track::Modeling` 显式豁免），
   这是设计选择，`spec_of(D9)` 查表可核，不是「忘了调闸」。

---

## 2 · 其余 AC 逐条

### AC-001 · TTL 合法且与模型视图一致 —— ✅ 通过

**独立解析（rdflib 7.6.0，不是只看 Dev 的脚本输出）**

```python
g = rdflib.Graph(); g.parse("model.ttl", format="turtle")      # 0 错误
```

| 计数 | rdflib `--model-view` | `ontology show --model-view` | 相等 |
| --- | --- | --- | --- |
| `owl:Class` | **83** | `entity_types` = 83 | ✅ |
| `owl:DatatypeProperty` | **866** | 属性总数 = 866 | ✅ |
| `owl:ObjectProperty` | **52** | `link_types` = 52 | ✅ |
| 三元组 | **5953** | — | — |

默认（客户视图）：`83 / 843 / 52`，**5472** 三元组。

**与参照物等价**（`code/out/ai-ontology.ttl`，5954 条）：

```text
ref=5954  ref-minus-created=5953  got=5953
isomorphic(ref − dcterms:created, model.ttl) = True
```

⇒ A-4 的「差一条 `dcterms:created`」成立。

**843 vs 866 的 23 条去哪了（实测，不采信报告口径）**

直接调用两个前处理函数（探针，真库）：

```text
model=866  projected=855  filtered=844
projection 丢 11 · filter 丢 11 · 同名合并 1
```

| 阶段 | 丢 | 证据 |
| --- | --- | --- |
| `view::project` 投影 | **11** | `projected=855` |
| `property_filter::filter_view` | **11** | `dropped.len()=11`，全部 `whole=true`（清单见附录 C） |
| `ttl.rs::distinct_properties` 同名合并 | **1** | 对象「试用请求」有 **2 个同名属性「请求标识」**（一条 required、一条 optional），TTL 按名字合并成 1 条（`required` 取或 ⇒ `ontok:nullable false`） |
| 合计 | **23** | 866 − 23 = **843** |

⚠️ **与 A-3 的「投影丢 12 + filter 剔 11」差 1**：第 23 条不是投影丢的，是 **TTL 出口自己的同名合并**。
因此「843 与 `/api/graph` 是同一份前处理，不是 TTL 的特殊行为」这句话**不成立**：
`/api/graph` 的客户视图 JSON 有 **844** 条属性（`ontology show` 实测 844），TTL 是 843。
不影响 AC-001 判据（AC-001 对的是 `--model-view` 的 866），但 A-3 的记账要订正 → **N-2**。

### AC-002 · 客户视图导出必须过守门器 —— ✅ 通过（按 A-1 口径）

| 场景 | 结果 |
| --- | --- |
| 正文说明含「主键」（**不含 ASCII 标识符**：`本表的主键服务中心，按业务口径解读`） | **CLI** `exit=2`、stdout **0 字节**、stderr 报词条；**HTTP** `GET /api/ontology.ttl`（带正确 token）= **500** |
| HTTP 500 响应体（180 B） | `{"error":"这一问不给数据：内容里出现了 1 项只有内部使用的术语（为免泄漏，此处不列出具体词条）。这是发布闸门，请联系维护者。"}` —— 不含「主键」、不含对象名、不含 `@prefix` |
| 清掉术语（对照组） | `exit=0`，stdout 1714 字节；HTTP 200 |
| `--out` | 失败时**不创建**目标文件 |

- 构造样例时**避开了 Dev 记的坑**（`"主键是 order_no"` 会被 `property_filter` 整段隐去、闸看不到），
  用的是不含 ASCII 标识符的形态，两条路（CLI/HTTP）都实测命中。
- 另外实测：**项目名**含 `外键`（`外键登记表_45900`）也会被拒（词条来自正文的 `ontok:sourceProject` / `rdfs:label`），
  证明「序列化后扫字节」这条设计确实覆盖了序列化自己引入的文本。

### AC-003 · 确定性 —— ✅ 通过

| 场景 | 结果 |
| --- | --- |
| `anspire_ai` v2 **客户视图**，两次独立进程 | `47C2F166689E0A0017F742CA41815BA4BA8525EE347D28D24FA9834BBB539D98`（两次相同，220,676 B） |
| `anspire_ai` v2 **建模视图**，两次独立进程 | `01016DB0B1F955D9832FC5DB73B529D21C162600BE2C0EABE9F501B9A23EE07C`（两次相同，231,973 B） |
| 正文里有没有时间戳 | 无 `dcterms:created`（A-4） |
| 反序输入 | `test_req110_ttl::ac003_two_exports_are_byte_identical_and_order_independent` 绿（输出只取决于集合） |

### AC-004 · HTTP 导出端点仍受门禁 —— ✅ 通过

真 socket（`secretproj` v9）：

| 请求 | 实测 |
| --- | --- |
| `GET /api/ontology.ttl` 无 token | **401**（60 B JSON，不含任何数据） |
| 错 token | **401** |
| 正确 token | **200** + `content-type: text/turtle; charset=utf-8` + `content-disposition: attachment; filename="secretproj-v9.ttl"` + `content-length: 1854`（与响应体字节数一致） |

（文件名口径双向钉住：真 socket 实测 `secretproj-v9.ttl`；单测 `ac004_…` 用 `demo3` ⇒ `demo3-v2.ttl`。）

### AC-005 · `/` 免 token 且不含任何本体数据 —— ✅ 通过（自查，未采信 Dev）

**服务现场**：`secretproj` / 第 9 版 / 含 `绝密对象`（`绝密说明`、`秘密字段`、`secret_col`）/ 另加 `公开对象`。

> ⚠️ 说明：库里的 `secretproj` 第 7 版已被**别的运行**占成空快照（7 个版本、快照 0 对象，且已冻结），
> 我在同项目上另建了第 9 版做本测试。本质不变（项目名/版本号/对象名都在）。

```text
GET /          无 token -> 200 · 12,479 字节
sha256(shell) = 96EB5FAE2CB444600CFB384AA31A3932986ED3E07F0C0C0A718E88A2A5CAD902
sha256(golden H1_index.html) = 与上面相同
body == page::index_html()  -> True（逐字节）
```

对响应体逐词 grep（全部 **0 命中**）：

| 词 | 命中 | 词 | 命中 |
| --- | --- | --- | --- |
| `secretproj` | 0 | `秘密字段` | 0 |
| `绝密对象` | 0 | `secret_col` | 0 |
| `绝密说明` | 0 | `top_secret` | 0 |
| `公开对象` | 0 | `@prefix` | 0 |
| `第 <b>9</b> 版`（正则 `第 <b>\d+</b> 版`） | 0 | `owl:Class` | 0 |
| `"version"` | 0 | `83`（任何计数的形态） | 0 |

壳里的数字只有模板常量（`1`/`0`/`2`/`5`/`91`/`14`/`86`… 全是 SVG 坐标与 CSS 值），没有任何计数形态。
额外确认：壳里**没有** token（`var TOKEN = ""`，只进内存）。

### AC-006 · 其它 API 仍全部 gated —— ✅ 通过

| 请求（无 token） | 实测 |
| --- | --- |
| `GET /api/graph?project=…&version=…` | **401** |
| `GET /api/node?…` | **401** |
| `GET /api/ontology.ttl` | **401** |
| `GET /api/scope` | **401** |
| `GET /api/definitely-not-a-route`（**未注册**） | **401** |
| `POST /api/xxx`（未注册） | **401**（不是 405） |
| `POST /api/graph` / `PUT /api/anything` / `PATCH /api/whatever` / `DELETE /api/node` | **401** |
| 错 token（graph / ttl / scope） | **401** |
| `POST /` / `DELETE /` | 405（只读语义，不泄露任何数据） |

⇒ `/api` 这一层的「路径存在性」仍然探测不到。

### AC-007 · CLI 与 HTTP 逐字节相同 —— ✅ 通过

| 场景 | CLI sha256 | HTTP sha256 | 相同 |
| --- | --- | --- | --- |
| `secretproj` v9（真 socket） | `FF74D74654989DEE37891D877256ECCA7D04E408288F359296E292790D914EBB` | 同左（**字节级比较** `True`） | ✅ |
| `anspire_ai` v2（真库） | `47C2F166…539D98`（两次进程相同，与 Dev 报告一致） | 同一路径：单测 `ac007_http_and_cli_ttl_bytes_are_identical` 在 `demo3` 夹具上比**字节**，绿 | ✅ |

**三项响应头断言**：`Content-Type` / `Content-Disposition` / `Content-Length` 见 AC-004。

**「共用同一个序列化函数」的否定式证据**（我自己 grep，不采信 Dev 的测试输出）：

```text
cli.rs    serialize_customer(=0   serialize_model(=0
http.rs   serialize_customer(=0   serialize_model(=0
pipeline.rs serialize_customer(=1   ← 正向对照，不是空转
```

### AC-008 · 有损项在文件头显式声明 —— ✅ 通过

文件头含 `ActionType` / `semantic` / `cardinality` 三个关键词，且**数量随数据走**：

```text
# · ActionType：未导出（4 个）—— OWL / SHACL / SWRL 均无干净对应（规格 §5.5）
# · semantic：降级为 rdfs:comment（语义只能作注释，不可推理）
# · cardinality：只映射 Functional / InverseFunctional（1:1 / 1:N / N:1；多对多无对应）
# · 不含实例数据（ABox）；不含 SHACL 形状（nullable / 必填不导出）
```

（`4 个` 来自快照里 4 条 ActionType；`anspire_ai` v2 是 `0 个`。）另外确认 `dcterms:created` 不写。

### AC-009 · 新出口必须接入 REQ-107 的判据 —— ✅ 通过

- `OUTLETS` **39** 条（33 + C6/C7/H4/H5/D9/D10），每条新出口都有 `path` + `call_site` needle + `exemption_reason`：
  - `C6` `emit_domain_text(customer_ttl)` · `C7` `fs::write(p, customer_ttl.as_str())`
  - `H4` `Ok((headers, rendered.into_text()))`（`WRITE_CONSTRUCTS` 已同步收口该形状）· `H5` `Ok(Json(rendered.into_value()))`
  - `D9` `emit_domain_text(model_ttl)` · `D10` `fs::write(p, model_ttl.as_str())`
- 分组 13/8/9/2/1 → **17/10/9/2/1**，逐项精确相等（`test_req107_outlet_pipeline` 12 绿）。
- REQ-107 的 9 个套件全绿（合计 41 条）：`outlet_pipeline 12` · `guard_granularity 4` · `zero_visible_change 3` · `pipeline_bytes 1` · `scope_error_gate 4` · `track_manifest 2` · `static_texts 3` · `red_lines 2` · `source_enumeration 10`。

### AC-010 · 改写而非删除 REQ-093 的判据 —— ✅ 通过（重点核对）

**① 主判据**：`unauthenticated_probes_cannot_discover_which_paths_exist`
→ `unauthenticated_probes_learn_nothing_but_the_shell_exists`（`src/server/http.rs:986`）。
**存在、未 `#[ignore]`、断言新行为。**

**② `/api/*` 断言一条不少**（逐条对账）：

| 基线断言 | 改写后 |
| --- | --- |
| `GET /api/graph` = 401 | ✅ 保留 |
| `GET /api/definitely-not-a-route` = 401 | ✅ 保留 |
| `POST /api/apply` = 401 | ✅ 保留 |
| `DELETE /` = 401（**根路径**） | ➜ 改为 `DELETE /api/node` = 401（根路径从 401 变 405，属允许的 `/` 变化） |
| — | ➕ `GET /` = **200** 且响应体含「业务关系图」、不含项目名 |

⇒ 基线的 3 条 `/api/*` 断言**加上** 1 条新的 `/api` 断言（共 4 条），**没有减少**。

**③ 其余 6 个被改写的测试**（逐个读 diff，全部「断言变强或等价」，无一条放宽）：

| 基线 | 改写后 | 强度 |
| --- | --- | --- |
| `without_token_every_endpoint_is_401`（3 URI） | `without_token_the_shell_is_public_but_every_api_is_401` | `/` 断言 401→200；API 由 2 条增至 **4 条**（+scope +ttl） |
| `a_hostile_project_name_cannot_break_the_page` | `…_the_shell_or_the_scope_json` | 「结构标记数量相等」→「**与干净壳逐字节相同**」+ scope JSON 原样还原（更强） |
| `a_project_name_with_modeling_terms_is_refused_on_the_homepage` | `…_on_the_scope_endpoint` | 增加「整个项目名也不许回显」（更强） |
| `a_clean_project_name_still_serves_the_homepage` | `…_still_serves_the_scope` | 增加 version 断言（更强） |
| `the_homepage_gate_does_not_bypass_the_token_gate` | `the_public_shell_does_not_open_the_scope_endpoint` | 语义更强 |
| `a_hostile_project_name_…`（`test_graph_server` / `test_exit_ordering_req106`） | 同名改写 4 条 | 见 diff：`index_html()` 无参、无占位符、无 `js_string_literal`（更强） |

**④ 有没有 `assert_eq!` → `assert!` 之类的悄悄放宽？** 没有。
`git diff 5737ecf..638c0dc` 里被删除的断言只有：`OUTLETS.len()==33`（→39）、`outlet_count(Customer)==13`（→17）、
`outlet_count(Modeling)==8`（→10）、`D1..D8`（→`D1..D10`）、以及 `contains("demo3")` 的页面断言（被更强的「逐字节等于干净壳」取代）。
没有新增 `#[ignore]`；测试函数名「删 6 / 加 6」全部是**成对改写**（已逐个对账，无孤儿删除）。

**⑤ H1 golden 重锚合规**：

| 项 | 结果 |
| --- | --- |
| `H1_index.html` 更新 | ✅ 新 sha256 `96eb5fae…` 与我实测的 `GET /` 字节**完全一致**（说明 golden 仍由生产路径产出） |
| `SHA256SUMS` 同步 | ✅ 仅 H1 那一行变化（1 行 diff） |
| `PROVENANCE.txt` 说明重锚原因 | ✅ 追加 7 行：R4/AC-005 要求静态壳 ⇒ H1 必然变；新 H1 = `INDEX_HTML` 常量；⛔ 不许手改 H1 |
| 其余 12 份 golden | ✅ **字节未动**（`git diff --name-only` 只有 H1/SHA256SUMS/PROVENANCE 三个文件） |
| 判据是否仍逐字节 | ✅ `test_req107_pipeline_bytes` / `test_req107_zero_visible_change` 仍比 golden，绿 |

### AC-011 · 不退化 —— ✅ 通过（除 B-1）

| 项 | 本轮实测 | 基线 | 判 |
| --- | --- | --- | --- |
| lib（含新增 15 条） | **245** | 230 | ✅ |
| doc | **4** | 4 | ✅ |
| 32 个基线回归套件 | **313**（逐套件见附录 B，全部 0 failed） | 313 | ✅ 一条不少 |
| 新增 `test_req110_ttl` | **14** | — | ✅ |
| 受影响的另两个套件 | `test_read_only_guard` 5 · `test_domain_print_sites_binary_req078` 4 | — | ✅ |
| 四红线 + `tools.rs` + `guard()` + `property_filter.rs` | `git diff --numstat` **为空** | — | ✅ |
| 新依赖 | `Cargo.toml` / `Cargo.lock` **零 diff** | — | ✅ |
| `#[ignore]` | 无新增（基线里已有的 3 处 `test_req107_red_lines` 等未动） | — | ✅ |
| `test_anchor_guard` | 基线 `5737ecf` 上 **2 passed / 1 failed**（`no_bare_env_manifest_dir_outside_anchor_file` 指向 `test_req109_cli_gate.rs:26`、`test_req109_e2e_fake_llm.rs:38`）；把 hotfix `dd71e21` cherry-pick 到临时分支后 **3 passed** | — | ⚠️ **不是 REQ-110 引入**，但 32 套件口径不含它（见 N-4） |
| 分支可构建性 | ❌ **B-1**：`src/bin/ontok-gate-preflight.rs` 在 `638c0dc` 中被删除，fresh checkout 下 `tests/test_retained_names_req084.rs` 编译失败 | — | ❌ |

---

## 3 · 变异测试（纪律 G：变异三元组，编译失败不算执行）

在 `qa-REQ-110`（`638c0dc`）上逐条打补丁 → 跑判据 → 还原（`git status` 确认工作区回到干净）：

| # | 变异（落在哪） | 编译/执行 | 被抓住它的判据（红） |
| --- | --- | --- | --- |
| M1 | `pipeline.rs` `CustomerTurtle` 分支：`gate_text(spec, doc.body())?;` → `let _ = doc.body();`（**关掉闸**） | ✅ 执行 | **5 红**：`test_req110_ttl` 的 `ac002_a_poisoned_note_is_refused…`、`ac002_the_gate_scans_the_serialized_bytes…`、`live_cli_export_refuses_a_poisoned_note_with_a_nonzero_exit`、`modeling_export_is_exempt_but_customer_export_is_not` + `http::tests::ac002_a_poisoned_note_is_refused_on_the_ttl_endpoint` |
| M2 | 同上：`doc.body()` → `doc.doc()`（**扫整份文件**，即 A-1 排除掉的形态） | ✅ 执行 | **7 红**：`ac002_a_poisoned…`、`ac003_two_exports…`、`live_anspire_ai_v2…`、`live_cli_export_is_byte_identical…`、`live_cli_export_refuses…` + `http::tests::ac004`、`ac007` |
| M3 | `http.rs::build_router`：摘掉 `/api` 子路由上的 `require_token` layer（**API 全公开**） | ✅ 执行 | **6 红**：`ac004`、`ac006`、`the_public_shell_does_not_open_the_scope_endpoint`、`wrong_token_is_401`、`unauthenticated_probes_learn_nothing_but_the_shell_exists`、`without_token_the_shell_is_public_but_every_api_is_401` |
| M4 | `get_ttl`：`Ok((headers, rendered.into_text()))` → 追加一个换行（**HTTP 侧第二份字节**） | ✅ 执行 | **1 红**：`ac007_http_and_cli_ttl_bytes_are_identical`（逐字节判据确实咬得住） |

**变异结论**：闸、扫描范围、身份门、CLI/HTTP 同字节四条关键不变式都有**在管线/路由层**咬得住的判据
（不是只测纯函数）。唯一暴露的判据强度问题见 N-3。

---

## 4 · 阻断项

### B-1 · `src/bin/ontok-gate-preflight.rs` 在提交里被整份删除（2758 行）

**事实链**

```text
$ git diff --numstat 5737ecf..638c0dc
0       2758    src/bin/ontok-gate-preflight.rs      ← 删除
$ git ls-tree develop  src/bin/ontok-gate-preflight.rs
100644 blob 2968a89…                                 ← develop 上是跟踪文件
$ git ls-tree HEAD     src/bin/ontok-gate-preflight.rs
（空）                                               ← 638c0dc 里不存在
$ git status --porcelain
?? src/bin/ontok-gate-preflight.rs                   ← 只在工作区，未跟踪
$ Get-FileHash src/bin/ontok-gate-preflight.rs
C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D   ← 与 Dev 报告一致
```

**后果（在 QA worktree 的 fresh checkout 上复现）**

```text
$ cargo test --test test_retained_names_req084 --no-run
error: couldn't read `tests\../src/bin/ontok-gate-preflight.rs`: 系统找不到指定的文件。 (os error 2)
  --> tests\test_retained_names_req084.rs:52
   | const SRC_PREFLIGHT: &str = include_str!("../src/bin/ontok-gate-preflight.rs");
error: could not compile `ontok` (test "test_retained_names_req084") due to 1 previous error
```

- Dev 报告 §8 说「挪开 → 跑完挪回 → sha256 前后一致」——**工作区确实一致**，但**提交里少了这个文件**；
  Dev 本地全绿是因为那个未跟踪副本还在，别人（CI / 评审 / PR）拿到的是删掉版本。
- 这不是 REQ-110 的功能改动，是「绕行手段」的副作用：**误提交了破坏性删除**。
- 同一提交里还有 `.feature/verification-report.md` 的 **8 行未提交**改动（anchor_guard 注记 + 另跑套件），PR 里看不到（见 N-7）。

**修复（在 feature 分支追加一个 commit，不改已推历史）**

```powershell
cd <repo>\feature-REQ-110
git add src/bin/ontok-gate-preflight.rs .feature/verification-report.md
git commit -m "[Fix] 恢复误删的 src/bin/ontok-gate-preflight.rs（绕行挪出后未重新入库）+ 报告注记 (关联: REQ-110)"
```

**验收**

```powershell
git ls-tree HEAD src/bin/ontok-gate-preflight.rs        # 非空
git status --porcelain                                  # 干净
git diff --numstat 5737ecf..HEAD -- src/bin/ontok-gate-preflight.rs   # 空（= 与基线一致）
```

> 修完后不影响 32 套件 313 的口径（该套件不在其中）；Linux CI 上 `cargo test` 也能重新编到它。

---

## 5 · 非阻断项

| # | 项 | 说明 | 建议 |
| --- | --- | --- | --- |
| **N-1** | **A-1 的措辞与代码不符**（§1） | 头部第 1 行含 `local_model(项目名)`、第 2 行含 `action_type_count`；不是「编译期常量、不含任何项目数据」。但这两个值**都与正文重复**，没有正文外泄漏面 | 1) 把 A-1 的理由订正为「头部动态字节与正文逐字重复，排除不减少覆盖」；2) 加固：闸扫 `prefix_block + body`（除有损声明块）；或加判据把头部钉成「常量模板 + `local_model(project)` + 计数」 |
| **N-2** | **A-3 的 23 条分解差 1**（§2 AC-001） | 实测 `11（投影）+ 11（filter）+ 1（TTL 同名合并）`；`/api/graph` 844 条属性 vs TTL 843。合并的是「试用请求 / 请求标识」（2 条同名属性，`required` 取或 ⇒ `nullable=false`） | 订正 A-3 的记账；把「同一对象内同名属性合并」登记为 TTL 出口的**已知有损项**（可选：写进文件头第 5 条声明）。不阻断（不影响 AC-001 的 866 对账） |
| **N-3** | 判据 `ac008_the_constant_header_is_outside_the_gated_bytes` **不经过管线** | 它直接调 `scan_modeling_terms(doc.body())`，钉的是「拆分性质」；变异 M2（管线改成扫 `doc.doc()`）下它**仍绿**（被别的判据抓住）。名字比实际强 | 补一条经 `render(C6, …)` 的判据：干净客户视图在「头部含 `cardinality`」时**仍能导出成功**（正对照），或直接断言管线里 `gate_text` 收到的是 `body` |
| **N-4** | **`test_anchor_guard` 基线红** | 基线 `5737ecf` 上 `no_bare_env_manifest_dir_outside_anchor_file` 失败（`test_req109_cli_gate.rs:26` / `test_req109_e2e_fake_llm.rs:38` 裸 `env!("CARGO_MANIFEST_DIR")`）；hotfix `dd71e21`（= `abf1c9f` 的一部分）修好后 **3 passed**（我 cherry-pick 到临时分支实测）。**不是 REQ-110 引入** | 把 `test_anchor_guard` 纳入「不退化」回归清单；REQ-110 合并基于 `abf1c9f` 即可（两边改动不重叠） |
| **N-5** | 失败导出**保留**已存在的 `--out` 文件 | `exit=2` 时才写文件 ⇒ 旧文件不被截断、不被清理（实测 `STALE` 原样保留）。不是违规（AC 只要求「不产出文件」），但脚本可能误读旧文件 | 报告里写明这个语义，或失败时给 stderr 提示「目标文件未改动」 |
| **N-6** | 项目名含术语时的**不对称** | `外键登记表_…` 项目：`GET /` **200**（壳零数据）、`/api/graph` **200**（图里没有项目名）、`/api/scope` **500**、`/api/ontology.ttl` **500**。于是页面（先取 scope）打不开、TTL 导不出，但手工敲 graph URL 能看图 | 在 ADR-004 或交付报告里补这句（fail-closed 的必然结果，不是缺陷）。顺带：`/api/scope` 是项目名唯一回显点，这条不对称值得写清 |
| **N-7** | `.feature/verification-report.md` 有 **8 行未提交** | 未提交的是「anchor_guard 基线红」注记 + 「另跑 5 个套件」；PR 里看不到 | 与 B-1 同一次 commit 一起入库 |
| **N-8** | 交付报告 §3 的 `isomorphic` 脚本用了 `code/out/ai-ontology.ttl` | 我独立复跑结果一致（True），但该参照物在 `code` worktree（`develop`），不在 feature 分支内 —— 换环境时脚本会找不到文件 | 无关紧要，报告里标注「参照物路径相对仓库根」即可 |

---

## 6 · 合并建议

**结论：当前不能合。** 修复 B-1（一条恢复提交 + 报告入库）后即可合，功能面**无需改代码**。

| 步骤 | 动作 |
| --- | --- |
| 1 | Dev 在 `feature/REQ-110` 追加恢复提交（`git add src/bin/ontok-gate-preflight.rs .feature/verification-report.md`），**不要**改写已推历史 |
| 2 | QA 复验（10 分钟）：`git ls-tree HEAD src/bin/ontok-gate-preflight.rs` 非空；`git status` 干净；`cargo test --test test_req110_ttl` = 14；`cargo test --test test_req107_source_enumeration` = 10；`cargo test --lib` = 245（Linux 上再跑一遍 32 套件 = 313） |
| 3 | BA 把 A-1 / A-3 的措辞订正（N-1 / N-2）落进 `acceptance.md`（**不影响代码**，可与合并并行） |
| 4 | 合并方式：`develop`（`abf1c9f`）→ `feature/REQ-110` 的 merge commit；两边改动文件不重叠（`test_req109_*` vs `page/http/ttl`），预期无冲突。合并后再跑一次 `test_anchor_guard`（应 3 passed） |
| 5 | 清理：`qa-REQ-110` worktree（分支 `qa/REQ-110`，**未 push**，无提交，可直接删）；`feature-REQ-110` 合并后按流程清 |

**风险判断**：功能面（11 AC）我按 P0 全量实测过，闸、字节一致性、零数据壳、判据改写都站得住；
唯一阻断是「误删跟踪文件」这一工程事故，**不是设计缺陷**，修复成本一条命令。

---

## 附录 A · 复现命令（QA 实测口径）

```powershell
# 独立 worktree（不改 feature 分支）
cd <repo-root>
git worktree add -b qa/REQ-110 qa-REQ-110 638c0dc

cd qa-REQ-110
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/ontok_demo'
$env:ONTOK_TEST_DATABASE_URL=$env:ONTOK_DATABASE_URL
cargo build --bin ontok

# AC-001 / AC-003：真库两种导出 + rdflib
.\target\debug\ontok.exe ontology export --project anspire_ai --version 2 --model-view --out qa-out\model.ttl
.\target\debug\ontok.exe ontology export --project anspire_ai --version 2 --out qa-out\customer.ttl
python qa-out\analyze.py     # rdflib 计数 + show --model-view 对账
python qa-out\isomorph.py    # isomorphic(ref − dcterms:created, model.ttl) = True

# AC-002 / 头部探针（真库 + 真进程，项目名/正文带术语）
cargo test --test qa_probe_req110 -- --nocapture --test-threads=1

# AC-004~007：真 socket
$env:ONTOK_VIEW_TOKEN='qa-req110-token-9'
.\target\debug\ontok.exe serve --project secretproj --version 9 --bind 127.0.0.1:8800
curl.exe -s -o NUL -w "%{http_code}`n" http://127.0.0.1:8800/api/ontology.ttl                     # 401
curl.exe -s -D qa-out\h9.txt -o qa-out\http-secret9.ttl -H "Authorization: Bearer qa-req110-token-9" `
     http://127.0.0.1:8800/api/ontology.ttl                                                        # 200
.\target\debug\ontok.exe ontology export --project secretproj --version 9 --out qa-out\cli-secret9.ttl
Get-FileHash qa-out\http-secret9.ttl,qa-out\cli-secret9.ttl     # 同一个 sha256

# 变异（4 组；每次跑完 revert，git status 干净）
python qa-out\mutate.py m1_gate_off apply        # → cargo test --test test_req110_ttl / --lib
python qa-out\mutate.py m2_scan_whole_doc apply
python qa-out\mutate.py m3_token_layer_off apply
python qa-out\mutate.py m4_http_second_impl apply
```

> 探针与脚本（`tests/qa_probe_req110.rs`、`qa-out/*.py`）只在 QA worktree 里，**未提交**。

## 附录 B · 测试计数逐项（全部 0 failed）

**① 32 个基线回归套件 = 313**（与 Dev 报告逐项一致）

| 套件 | passed | 套件 | passed | 套件 | passed |
| --- | --- | --- | --- | --- | --- |
| `test_req107_outlet_pipeline` | 12 | `test_snapshot_write_paths` | 8 | `test_diff` | 13 |
| `test_req107_guard_granularity` | 4 | `test_proposal` | 9 | `test_no_write_path` | 4 |
| `test_req107_zero_visible_change` | 3 | `test_model_view` | 10 | `test_provenance_source` | 5 |
| `test_req107_pipeline_bytes` | 1 | `test_view` | 26 | `test_overview` | 17 |
| `test_req107_scope_error_gate` | 4 | `test_review_show_e2e` | 11 | `test_req108_live` | 5 |
| `test_req107_track_manifest` | 2 | `test_review_state` | 28 | `test_req108_confidence` | 10 |
| `test_req107_static_texts` | 3 | `test_db` | 11 | `test_req107_source_enumeration` | 10 |
| `test_req107_red_lines` | 2 | `test_extracted_by_provenance` | 9 | `test_property_filter` | 10 |
| `test_exit_ordering_req106` | 7 | `test_review_provenance` | 17 | `test_concept_proposal` | 19 |
| `test_binary_output_guard` | 7 | `test_graph_server` | 9 | `test_apply` | 21 |
| `test_guard_manifest_req081` | 6 | `test_snapshot` | 10 | | |

**② 本轮新增 / 另跑（不在 313 口径内）**

| 套件 | passed | 性质 |
| --- | --- | --- |
| `test_req110_ttl` | **14** | 本轮新增 |
| `test_read_only_guard` | 5 | 受 `require_token` 收窄影响，另跑 |
| `test_domain_print_sites_binary_req078` | 4 | 受 CLI 改动影响，另跑 |
| `test_anchor_guard` | **2 passed / 1 failed** | 基线 `5737ecf` 上本就红（REQ-109 两处裸 `env!`）；hotfix `dd71e21` 下 **3 passed**（见 N-4） |

**③ lib / doc**：`cargo test --lib` = **245**（0 failed）；`cargo test --doc` = **4**（0 failed）。

## 附录 C · `property_filter` 剔除的 11 条（AC-001 的分解证据）

```text
客户功能权限 [功能编码，如 jd_api]                         whole=true
客户订阅     [计费周期：monthly/quarterly/semi_yearly/yearly] whole=true
兑换码       [核销人customer_id]                           whole=true
站内信       [消息类型：transaction/rule_alert/system/recharge] whole=true
大模型服务商 [供应商配置(base_url, api_key等)]              whole=true
大模型服务商 [关联模型ID(对应llm_model.model_id)]           whole=true
订单         [计费周期：monthly/quarterly/semi_yearly/yearly（套餐订单使用）] whole=true
产品目录     [产品标识，如 llm_token、sentiment]            whole=true
产品目录     [产品类型：llm / api_service / agent]          whole=true
套餐模板     [发售总量上限（REQ-008）：…]                   whole=true
用量明细     [关联的API Key ID]                            whole=true
```

> 另有 2 条对象说明被隐去（授权额度 / 模型排序），那是 `DropKind::SilencedNote`，不减属性条数。
