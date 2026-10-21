# REQ-115 · 概念分组与图上的归并 —— QA 复审（第 2 轮 · C 裁定复验）

> **被测**：`feature-REQ-115` @ `cc7e44a`（分支 `feature/REQ-115`，2 个提交）
> **基线**：`develop` @ `71ae770`
> **独立变异 worktree**：`qa-REQ-115-2`（`qa/REQ-115-2`，从 `cc7e44a` 建）+ `qa-baseline-115`（detached `71ae770`）—— 审核完**已移除**，分支已删
> **环境**：Windows · PostgreSQL 16.15 · `postgresql://postgres:***@172.17.0.87:5432/anspire_real`
> **未改 feature 分支、未 push**（复核：`feature-REQ-115` 工作树 `git status` 为空，HEAD 仍 `cc7e44a`）
> **日期**：2026-10-22

---

## 1 · 结论

**PASS 可合并。**

上一轮的阻断（B-1：`scope_hint` 含「Key」⇒ 整份 `/api/graph` 500）**已修复**：现在同一句在 `concept import` 就被点名拒绝，库不变；把「未持 Key」改成「未持凭证」后走完整链路，图 **200 / 8 组 / 83 节点**，业务总览 **exit 0**（上一轮这里是 exit 2）。

C-1 / C-2 / C-3 三条裁定**全部实测兑现**。2 条指定变异**均被抓**（3 红 / 4 红）。

另有 **3 条非阻断**需要在下一轮或文档上收口，其中 1 条（旧快照仍会 500）**必须**在文档里纠正措辞，否则会留下一个「承诺比实现强」的坑：

| # | 级别 | 事项 |
| --- | --- | --- |
| **P1-1** | 文档 / 下一轮 | 闸只挡**新导入**。C-1 之前已经进库的脏 `scope_hint` 快照，出图仍 500、总览仍 exit 2（现库中仅 1 行，见 §9.1）。`graph.rs` 与 `PROVENANCE.txt` 写的「不会出现『快照里有脏说明、出图时整份 500』」**对旧数据不成立**，建议改成「新导入不会」。 |
| **N-1** | 测试覆盖 | 闸的**接线**（`cli.rs` 调用点）没有测试钉住：把调用点拆掉（函数留着），`--lib` 290 全绿、`test_req115_groups` 13 全绿 —— 见 §7.3。 |
| **N-2** | 判据口径 | P0 举的例子 `api_key_group` **不会**被拒（也不会打挂图，实测 200）。见 §2.4：这不是漏判，是「命中」的定义问题。 |

| AC | 结果 | 关键证据 |
| --- | --- | --- |
| C-1 | ✅ | `display_name` / `scope_hint` / 组 `key` 三字段命中即拒，报「组 + 字段 + 词条」；拒后 `proposals` 计数不变、连项目行都不建 |
| C-2 | ✅ | `concept suggest` 两次逐字节相同（`abc9068a…`），9 组 `scope_hint` 全 `null`；库四表指纹前后一致 |
| C-3 | ✅ | 图与总览 8/8 组的说明**逐字相同**；两个出口都带 |
| AC-005 | ✅ | 修好的声明 `import → 签 → apply → freeze → GET /api/graph` = **200 / groups=8 / 83 节点 / node.group 全非空** |
| AC-008 | ✅ | `INDEX_HTML` = `H1_index.html` = 真 HTTP `GET /`，三方同 sha `c8d5e148…`（49936 B）；13/13 `SHA256SUMS` 命中；其余 12 份逐字节未动 |
| AC-011 | ✅ | lib **290**/0、doc **4**/0、`test_req115_groups` **13**/0；3 个既有红灯在 `71ae770` 上同样红（同一批测试名）；禁改面 diff 为空 |

---

## 2 · C-1 · 三种拒绝 + 「碰库之前」

### 2.1 三个客户可见字段，各拒一次

测试项目 `qa2_r115_main` = 从 **`anspire_excl` v2（0 组 / 84 对象 / 52 关系）** 复制出的 v1（内容 hash 与源**完全相同**，见 §2.5）。

每次拒之前/之后**同一秒内**取全库计数：

```text
BASELINE  projects=1058 | ontology_versions=1322 | proposals=530 | proposal_items=1738 | concept_groups=284
```

| # | 声明 | exit | 报错（原文） | 库计数 |
| --- | --- | --- | --- | --- |
| ① | AI 原声明（`api_entitlement.scope_hint` 含「未持 **Key**」） | **1** | `组 api_entitlement 的 scope_hint 含建模术语 key` | **不变** |
| ② | 修好的声明 + `display_name` 改成「未持 Key 的授权」 | **1** | `组 api_entitlement 的 display_name 含建模术语 key` | **不变** |
| ③ | 修好的声明 + 组 `key` 改成 `primary_key` | **1** | `组 primary_key 的 key 含建模术语 primary_key` | **不变** |
| ④ | 三个字段都干净（「未持凭证」） | **0** | 导入提案 `p1`，8 个概念组 | +1 提案 / +8 条目 |

①的完整原文（②③同形，只换字段名与词条）：

```text
✗ 概念声明被拒绝导入：有 1 处组字段会撞上客户发布闸（这些字段会出现在 /api/graph 的 groups[] 与业务总览的 summary 里）——
  - 组 `api_entitlement` 的 scope_hint 含建模术语 `key`
请改措辞后重算指纹（ontok concept digest --file …）再导入。⛔ 不会静默剔除，也不会替你改写。
```

- 报错**点名组、字段、词条**，并给出**可操作的改法**（改措辞 → 重算指纹 → 再导入），且**不替作者猜业务措辞** —— 符合 A-8 的处置要求。
- 没有放宽 `guard()`，没有静默剔除：声明文件本身一个字节都没被改写（拒绝后同一份文件再跑仍是同样的错）。

### 2.2 「碰库之前」—— 自己查的，不采信注释

两种独立证据：

**(a) 代码顺序**（`src/cli.rs:1201–1226`）：

```text
读文件 → parse → decl.verify() → scan_group_names() ⇒ 命中就 bail!
        ↓（只有全干净才往下走）
PgProjectStore::new / get_or_create / version_by_seq / proposals.create
```

闸在 `PgProjectStore::new` **之前**，即整条 import 路径上**没有任何一次查询**发生在判决之前。

**(b) 行为证据**：拿脏声明打一个**根本不存在的项目名** `qa2_never_created`：

```text
EXIT=1
✗ 概念声明被拒绝导入：… 组 `api_entitlement` 的 scope_hint 含建模术语 `key`
BEFORE: projects=1058 | ontology_versions=1322 | proposals=530 | proposal_items=1738 | concept_groups=284
AFTER : projects=1058 | ontology_versions=1322 | proposals=530 | proposal_items=1738 | concept_groups=284
UNCHANGED=True
select count(*) from projects where name='qa2_never_created'  →  0
```

**项目行都没被创建** ⇒ 连 `get_or_create` 都没跑到，不只是「没写提案」。这比「proposals 计数不变」更硬。

### 2.3 干净声明确实能进

```text
ontok concept import --file v_clean.json --project qa2_r115_main --base-version 1
  → 提案 p1，8 个概念组（客户与子账号 / 商品、套餐与订阅 / 用量、扣费与余额 /
    API 密钥与授权 / 模型、服务商与服务定价 / 营销、活动与兑换码 /
    智能体实例与任务 / 后台与运营支撑）
```

### 2.4 ⚠️ P0 的 `api_key_group` 例子：**不拒，但也不危险**（实测）

P0 写的是「组 `key` 含 `key`（例如 `api_key_group`）⇒ 拒」。实测**不拒**。把整张真值表打出来（同一份 `scan_group_names`）：

| 组 `key` | 拒？ | 命中词条 |
| --- | --- | --- |
| `key` | ✅ 拒 | `key` |
| `primary_key` | ✅ 拒 | `primary_key` |
| `group key` | ✅ 拒 | `key` |
| `主键组` | ✅ 拒 | `主键`（中文包含匹配） |
| `api_key_group` | ❌ **放行** | — |
| `api_key` | ❌ 放行 | — |
| `api-key` | ❌ 放行 | —（`api` 在业务白名单） |

**为什么不是漏判**：`scan_modeling_terms` 的分词把 `_` 当**词内字符**（`c.is_alphanumeric() || c == '_'`），所以 `api_key_group` 是**一个词元**，不等于守门词 `key`；而 `key` / `primary_key` / `group key` / `主键组` 都能命中。这正是 A-1 记的「`License Key` 不误伤」的同一条规则，不是 `key` 字段没扫（`primary_key` 那条就是 `key` 字段被抓的证据）。

**为什么也不危险**（关键：导入闸与出图闸是**同一把尺子**）：

```text
# 真跑完一整条链路（导入 → 双签 → apply → freeze），组 key 就叫 api_key_group
GET /api/graph?project=qa2_r115_key&version=2   → 200
  83 节点 / 52 关系 / 8 组 / node.group=null 0/83
  groups[].id 里有 "api_key_group"（值），响应里仍然没有字面键 "key"
ontok ontology overview --project qa2_r115_key --version 2          → exit 0
ontok ontology overview --project qa2_r115_key --version 2 --json   → exit 0
```

因为导入判定与出图判定调用的是**同一个函数、同一批值**，所以「过得了 import」⇒「不会打挂图」。要把 `api_key_group` 也拒掉，就得让导入闸**比客户闸更严**，那会拒掉客户闸本来放行的名字 —— 与 C-1「客户可见字段过客户闸」的原意相反。

**判定**：C-1 的**要求**（三字段都过 `scan_modeling_terms`，命中即拒）**已兑现**；P0 里 `api_key_group` 那半句**基于对「命中」的错误模型**（子串 vs 词元）。若 BA 仍要求「key 字段出现 `key` 字样就拒」，那是**放宽口径之外的新要求**（会误伤 `api_key` / `api-key` / `license_key` 这类业务名），需单独裁定，不建议在本轮顺手加。

### 2.5 副本与源是同一份快照

```text
qa2_r115_main v1 ← anspire_excl v2
object_types  84  内容 hash 5ae563be5221f7bcb64b9504073806e4（dst == src）
link_types    52  内容 hash 59a2e9f3a0d440a5351a40cf1322c795（dst == src）
action_types   0 / functions 0
```

（去掉 `id`/`version_id`/`project` 后的整行文本 hash，两边逐行相同。）

真实项目 `anspire_excl` / `anspire_real` 仍是 **0 组**，未被本次审核写入（见 §9.3 清理记录）。

---

## 3 · C-2 · `concept suggest` 不再写客户字段

在 `anspire_excl` v2（0 组，真实项目）连跑两次：

```text
sha256(suggest_1.json) = ABC9068AC018B93ADD267D17543489A753C8C72B2A0175E8CF31723F9654E953  (3722 B)
sha256(suggest_2.json) = 同上
byte-identical = True
version=1.0.0  declared_by='concept-suggest/greedy-modularity-v1'
source='algorithm-suggest'  reviewed_by=None  confirmed_by=None
rule='按关系拓扑做确定性贪心模块度分组（整数打分、无随机数；同输入 ⇒ 同输出；单成员不成组）'
groups=9  members_total=45
scope_hint 取值 = [None × 9]        ← 9/9 全 null
```

- **内部备注去哪了**：`declared_by`（来源）+ `rule`（算法与不变量）。这两处不进客户视图，正好承载「算法建议 / 待人审」，符合 C-2。
- 旧常量 `SCOPE_HINT`（「算法建议：按关系拓扑自动分组，待人审」）已删除，全仓不再有对它的引用；也没有任何测试/夹具还在期待那句旧文案。
- **确定性仍在**：两次运行逐字节相同。注意 sha 与上一轮不同（上一轮 `dff0e6eb…`）—— 那是**删掉 `scope_hint` 后的必然变化**，不是确定性退化。
- **产物自己过得了 C-1 的闸**：把 `suggest_1.json` 原样导入一个空项目 ⇒ `exit 0`，提案 9 个概念组（`import EXIT=0`）。

**库指纹（跑前 / 跑后四个 hash 全部相同）**：

```text
                rows   content-sha256(前16)
concept_groups  300    a50cc2f45fc2aa40
ontology_versions 1324 54743da283ba047e
proposals       532    ecb2376838293b9c
proposal_items  1754   a47fdd3d83184cda
BEFORE == AFTER: True
```

（行数含本次审核在跑的其他 QA 夹具，因此不是「干净基线」的绝对值；判据是**前后完全相同**。四表按「去掉 id/version_id/project_id/proposal_id 的整行文本」排序后拼接取 SHA-256。）

---

## 4 · C-3 · 图与总览同口径 + 两个出口

### 4.1 客户图（修好的声明 · 完整链路）

```text
concept import(v_clean) → p1 → review decide --accept-all → apply → v2 → freeze
GET /api/graph?project=qa2_r115_main&version=2   → 200   (19853 B)
  nodes=83  edges=52  groups=8  node.group=null 0/83
  响应里出现字面键 "key"？ False
  scope_hint 在？ True（8/8 组都有）
  组明细：customer_account 7 / catalog_subscription 11 / billing_usage 15 /
          api_entitlement 8 / model_provider 11 / marketing_growth 10 /
          agent_instance 7 / admin_ops 14 = 83
```

### 4.2 业务总览（上一轮 exit 2 的地方）

| 项目 / 版本 | `ontology overview` | `--json` | 说明 |
| --- | --- | --- | --- |
| `qa2_r115_main` v2（修好的 8 组） | **exit 0** | **exit 0** | 本轮新增的关键判据 |
| `qa2_r115_key` v2（key=`api_key_group`） | exit 0 | exit 0 | |
| `anspire_excl_r115` v4（修好的） | exit 0 | exit 0 | Dev 留下来的已验证副本 |
| `anspire_excl` v2（真实项目，0 组） | exit 0 | exit 0 | 退化成对象清单 |
| `anspire_real` v2（真实项目，0 组） | exit 0 | exit 0 | |
| `anspire_excl_r115` v3（旧脏快照） | **exit 2** | **exit 2** | `⚠ 客户视图泄露了建模术语（出口 C4/C5）：["key"]`，见 §9.1 |

### 4.3 两个出口的说明文字**逐字相同**

按组名配对，`GET /api/graph` 的 `groups[].scope_hint` 与 `ontology overview --json` 的 `concepts[].summary`：

```text
qa2_r115_main v2 : 8 组 · SAME 8/8（graph 8 组都带 · overview 8 组都带 · 组名集合相同）
qa2_r115_key  v2 : 8 组 · SAME 8/8
anspire_excl_r115 v4 : 8 组 · SAME 8/8
```

**C-3 成立**：选的是「都带」，且带的是**同一个来源的同一段文本**（都是声明的 `scope_hint`），不是各写一份。

### 4.4 页面也照常显示

`page.rs` 的组面板保留 `g.scope_hint` 渲染（`H1` 里可见），`tests/test_req115_groups` 用 `html.contains("g.scope_hint")` 钉住。折叠/展开与分组渲染未改动。

---

## 5 · 审核界面仍看得见 `scope_hint`（内部轨）

`ontok review serve`（REQ-111，回环 + Bearer）对一个**未冻结**的提案（修好的声明）：

```text
GET /api/review  + Bearer        → 200
GET /api/review  无口令          → 401
GET /            无口令          → 200（页面壳）
items = 8，items carrying payload.scope_hint = 8/8
payload keys = ['display_name','evidence','extracted_by','key','members','scope_hint']
第一组 payload.scope_hint（原文，内部轨）：
  客户主档、账号绑定关系、客户内部员工与子账号权限、客户功能开关。dim_customer_binding（账号渠道归属）与
  internal_staff 归此组，因为两者都是「这个客户是谁、他下面还有哪些人」的归属维度信息；若有异议可移出……
```

**「内部看得见原文、客户看到的是过过滤的」这句话成立**：审核界面直接读提案 `payload`（不经过客户闸），客户出口读的是**导入时已过闸**的同一段文本。两轨的分界是**入口**，不是展示层。

---

## 6 · golden

| 检查 | 结果 |
| --- | --- |
| `INDEX_HTML` 常量（`page.rs`，从 `r##"…"##` 原样切出） | 49936 B · `c8d5e148a647d84b81d2d1186430dfb6455979f786725a8a5bcd714548ed7c51` |
| `tests/fixtures/req107/golden/H1_index.html` | 同 sha · 同字节 |
| 真 HTTP `GET /`（`curl.exe -o` 原始字节，避开 PowerShell 重编码/BOM） | 同 sha · 同字节 |
| `SHA256SUMS` 13 行逐条复核 | **13/13 命中** |
| 其余 12 份 golden 与 `develop` 逐字节 | **未动**（`develop...HEAD` 只改了 `H1_index.html` / `PROVENANCE.txt` / `SHA256SUMS`） |
| 本提交对 `H1` 的增量 | **只 +2 行注释**（`showGroups()` 里说明 `scope_hint` 是客户可见说明），无行为改动 |
| `PROVENANCE.txt` | 写明第二次重锚与 C-1/C-2/C-3 的理由；含「REQ-115」「分组」 |

---

## 7 · 变异（新判据有牙）

全部在独立 worktree `qa-REQ-115-2` 上做，**每次跑完都还原并核对**：`src/bin/ontok-gate-preflight.rs` 的 sha256 是 `C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D`、`Test-Path=True`、无 `.parked-by-qa` 残留；`git status --porcelain` 只剩我自己那份临时探针文件（已删）。编译失败不算执行 —— 三次变异都编译通过并跑出结果。

### 7.1 变异 A：只扫 `display_name`（`scope_hint` 与组 `key` 都跳过）

```text
cargo test --lib            → 288 passed / 2 failed
  RED test_group_key_itself_is_scanned_as_a_wire_value
  RED test_scope_hint_with_a_modeling_term_is_rejected
cargo test --test test_req115_groups → 12 passed / 1 failed
  RED group_fields_are_checked_before_import_not_when_serving
```

**被抓（3 红）。** 但注意：这条变异**没有**被「只扫 display_name 就会漏掉 key」以外的第三个测试抓到 —— 现有测试对「三个字段都要扫」是分字段钉的，不是端到端。

### 7.2 变异 B：`scan_group_names` 直接 `return Vec::new()`

```text
cargo test --lib → 287 passed / 3 failed
  RED test_group_key_itself_is_scanned_as_a_wire_value
  RED test_group_display_name_with_a_modeling_term_is_named_precisely
  RED test_scope_hint_with_a_modeling_term_is_rejected
cargo test --test test_req115_groups → 12 passed / 1 failed
  RED group_fields_are_checked_before_import_not_when_serving
```

**被抓（4 红）。**

### 7.3 变异 C（额外）：把**调用点**拆掉（函数留着，`cli.rs` 不再调）

```text
cargo test --lib                     → 290 passed / 0 failed   ← 全绿
cargo test --test test_req115_groups → 13 passed /  0 failed   ← 全绿
```

**没被抓。** 这说明「闸存在且判得对」有测试，「闸真的接在 import 上」没有测试。今天靠代码审查保证接线，不靠测试。见 §9.2 的建议。

---

## 8 · 不退化

### 8.1 测试数字

| 套件 | feature `cc7e44a` | baseline `71ae770` |
| --- | --- | --- |
| `cargo test --lib` | **290 passed / 0 failed** | 272 passed / 0 failed |
| `cargo test --doc` | **4 passed / 0 failed** | —— |
| `--test test_req115_groups`（新增） | **13 passed / 0 failed** | 不存在 |
| `--test test_req107_outlet_pipeline` | 12 passed / 0 | —— |
| `--test test_req107_pipeline_bytes` | 1 passed / 0 | —— |
| `--test test_req107_zero_visible_change` | 3 passed / 0 | —— |
| `--test test_req107_source_enumeration` | 10 passed / 0 | —— |
| `--test test_graph_server` | 9 passed / 0 | —— |

> **环境提示**：`--lib` 里有 2 条测试（`db::apply::…unique_conflict…`、`extract::introspect::…read_only_pool…`）**必须以 `ONTOK_TEST_DATABASE_URL` 运行**；不设该变量时它们会因为「缺环境变量」而红（报 `MissingEnv`），设上就是 290/0。这不是代码缺陷，但会让复跑者误判 —— 建议 Dev 在验证报告里写清这个前置条件。

### 8.2 三条既有红灯：feature 与基线**同红同一条**

| 套件 | feature `cc7e44a` | baseline `71ae770` | 归因 |
| --- | --- | --- | --- |
| `drop_rate` | 0 passed / 1 failed（`real_schema_drop_rate_report`） | 0 / 1（同一测试） | 缺 `.review-workspace/fix-props/schema.json`（未纳入版本控制的脱敏 schema，fail-closed） |
| `test_fk_naming` | 5 / 1（`test_real_schema_customer_id_has_exactly_one_display_name`） | 5 / 1（同一测试） | 同一份 schema 缺失 |
| `test_agent_orchestration` | 35 / 1（`test_cli_end_to_end_output_is_byte_identical_to_legacy_extract`） | 35 / 1（同一测试） | REQ-108 分档路由导致的既有 CLI 端到端字节差异，与 REQ-115 无关 |

（基线侧同样临时移开 unix-only 的 `ontok-gate-preflight.rs`，`finally` 挪回：`sha256=C7CF95E2…`、`Test-Path=True`、无残留。）

> 另：不设 `ONTOK_TEST_DATABASE_URL` 时 `test_agent_orchestration` 会多红一条，容易被误读成「REQ-115 多了一处退化」；设上环境变量后与基线完全一致。

### 8.3 禁改面

`git diff --stat develop...HEAD` 对以下路径**为空**：

```text
src/domain/property_filter.rs
src/mcp/**                 （含 tools.rs）
src/domain/view.rs
src/domain/review_view.rs
src/extract/**
Cargo.toml · Cargo.lock
```

`src/server/http.rs` 有改动（+79/−9），逐 hunk 核对后**只涉及分组管道**：

- `load_frozen_view` 多返回第三项「已声明的概念组」（与客户视图**同一份快照**取出）；
- `state_from_view` 拆成 `state_from_view` / `state_from_view_with_groups` / 共用的 `state_from_view_inner`（差别只有一行：用哪个投影函数）；
- `serve` 改走带分组的入口；
- 测试夹具补 `groups: None` / `group: None`。

**没有改** `build_router` 的路由表、令牌校验、401/405 语义。`tools.rs` / `property_filter.rs` 一个字节没动，也没有新增依赖。

### 8.4 页面与鉴权（真请求）

```text
GET /                              → 200
GET /api/graph       无 token      → 401
GET /api/node        无 token      → 401
GET /api/scope       无 token      → 401
GET /api/ontology.ttl 无 token     → 401
GET /api/nope        无 token      → 401（未注册路径也不泄露存在性）
GET /api/graph       错 token      → 401
POST /api/apply      无 token      → 401
POST /api/apply      带 token      → 405（只读语义不变）
```

- 页面 **49936 B**（< 80KB），`http://` / `https://` / `<script src` / `<link` / `@import` / `cdn.` / `fonts.` **全 0**。
- `GET /` **零数据壳**：页面字节里「客户余额 / 套餐模板 / 活动 / 授权额度 / 用量明细 / 兑换码 / 客户订阅 / 大模型服务商」各 0 次。

---

## 9 · 仍需修的地方（按级别）

### 9.1 🟠 P1-1 · 闸只挡新导入；旧快照仍会 500 / exit 2，而文档把话说满了

**事实**（可直接复现）：

```text
anspire_excl_r115 v3（AI 原声明那条，`api_entitlement.scope_hint` = 「…未持 Key 的试用请求」）
  GET /api/graph?…&version=3  → 500
  stderr: ✗ mcp-server 守门器命中 1 项建模术语，已拒绝外送：["key"]
          ✗ 只读图服务守门器命中建模术语，已拒绝出图。
  ontology overview --version 3       → exit 2（C4）
  ontology overview --version 3 --json→ exit 2（C5）
```

- **不是本轮引入的**：`cc7e44a` 对读路径**没有任何功能改动**（`graph.rs` 只加注释；`overview.rs` 完全没改），所以 v3 在 `7718061` 上同样是 500 / exit 2。C-3 选「都带」并没有让旧数据变差，只是把「图安全、总览红」的**不一致**变成了「两边一致地红」。
- **影响面实测**：审核期间全库 300 行 `concept_groups`（含我自己的干净副本）逐行按 C-1 的三字段口径过闸，**只有 1 行**命中（就是 `anspire_excl_r115` v3，本轮之前就存在的验证副本）；副本清掉后全库 284 行，命中的仍是那 1 行。真实项目 `anspire_excl` / `anspire_real` **0 组**，不受影响。
- **要改的是措辞，不是闸**。现在两处文档给了超出实现的保证：
  - `src/domain/graph.rs` 的 `Group` 文档：「…不会出现『快照里有脏说明、出图时整份 500』」；
  - `tests/fixtures/req107/golden/PROVENANCE.txt`：「之所以不再有『一句话打挂整份图』的风险…」。
  建议改成「**新导入**不会出现…；`cc7e44a` 之前入库的快照不在保护范围内」。
- **下一轮可选的兜底**（不在本轮）：出图/总览读取时对 `scope_hint` 做一次 fail-closed 检查并**只拒该组、不拒整份响应**（现在整份 500 的粒度太粗）；或写一条一次性迁移，把历史快照里的脏 `scope_hint` 找出来交人改。

### 9.2 🟡 N-1 · 闸的接线没有测试

变异 C（§7.3）证明：把 `cli.rs` 的调用点删掉，两套测试**全绿**。建议在 `tests/` 里加一条**真进程**测试：拿一份 `scope_hint` 带 `key` 的声明 `concept import` 到一个新建项目，断言 `exit != 0` + stderr 含「组 + 字段 + 词条」+ 库里 `proposals` 行数不变。这样「闸被拆线」才会红。

### 9.3 🟡 N-2 · P0 的 `api_key_group` 期望与实现不一致（口径问题，不是缺陷）

见 §2.4。C-1 的**要求**已满足（三字段都过同一个闸），但 P0 举的例子按「子串包含」理解，与 `scan_modeling_terms` 的「词元相等」语义不同。**实测该 key 既不被拒、也不会打挂图/总览**（200 / exit 0）。需要 BA 明确：是接受现状（推荐），还是把「含 key 字样就拒」立成新要求（会误伤 `api_key` / `api-key` / `license_key`）。

### 9.4 🟡 N-3 · 无真浏览器手测（一如既往）

组圈观感、折叠动画、组心引力手感仍只有源码级 + Node 探针 + 真数据计数，没有浏览器截图。本机无 headless 浏览器，不为审核新增依赖。**未变化**。

### 9.5 🟡 N-4 · `scope_hint` 一字段两用途（已登记，下一轮）

BA 的 A-9 已登记（拆 `scope_hint` + `review_note`）。本轮的两条缓解（导入过闸 + suggest 不写）实测有效，但**不是**根本解；建议下一轮连同 §9.1 一起做。

---

## 附 · 复跑入口

```powershell
# 环境
$env:ONTOK_DATABASE_URL      = 'postgresql://postgres:***@172.17.0.87:5432/anspire_real'
$env:ONTOK_TEST_DATABASE_URL = 'postgresql://postgres:***@172.17.0.87:5432/anspire_real'   # --lib 必需
$env:ONTOK_VIEW_TOKEN        = '至少 8 个字符'

# 独立 worktree（本轮用过的，已移除）
git worktree add qa-REQ-115-2 -b qa/REQ-115-2 feature/REQ-115
git worktree add --detach qa-baseline-115 71ae770

# C-1（三种拒绝；每个都先记 proposals 计数）
ontok concept import --file code/out/concept-declaration-ai.json --project <副本> --base-version 1   # exit 1
ontok concept import --file <display_name 含 Key>.json           --project <副本> --base-version 1   # exit 1
ontok concept import --file <组 key = primary_key>.json          --project <副本> --base-version 1   # exit 1
ontok concept import --file <三个字段都干净>.json                 --project <副本> --base-version 1   # exit 0

# C-2
ontok concept suggest --project anspire_excl --version 2 --out s1.json
ontok concept suggest --project anspire_excl --version 2 --out s2.json   # sha256 相同、scope_hint 全 null

# C-3 + 两个出口
ontok review decide --proposal p1 --accept-all --project <副本>
ontok proposal apply --proposal p1 --project <副本>        # → v2
ontok version freeze --project <副本> --version 2
ontok serve --project <副本> --version 2 --bind 127.0.0.1:18871
curl.exe -s -D h.txt -o g.json -H "Authorization: Bearer $env:ONTOK_VIEW_TOKEN" "http://127.0.0.1:18871/api/graph?project=<副本>&version=2"
ontok ontology overview --project <副本> --version 2
ontok ontology overview --project <副本> --version 2 --json

# 审核界面（内部轨）
ontok review serve --project <副本> --proposal p1 --bind 127.0.0.1:18878
curl.exe -s -H "Authorization: Bearer $env:ONTOK_VIEW_TOKEN" http://127.0.0.1:18878/api/review

# 测试（必须先挪开 unix-only 的 preflight，finally 挪回）
Move-Item src/bin/ontok-gate-preflight.rs src/bin/ontok-gate-preflight.rs.parked-by-qa
cargo test --lib ; cargo test --doc ; cargo test --test test_req115_groups
Move-Item src/bin/ontok-gate-preflight.rs.parked-by-qa src/bin/ontok-gate-preflight.rs
#   sha256 = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D ; Test-Path = True
```

### 本轮留下的库状态

- 审核用的副本 `qa2_r115_main` / `qa2_r115_key` / `qa2_r115_review` / `qa2_r115_sugg` **全部已删除**（每个连同其版本、概念组、提案、条目）。
- 未创建 `qa2_never_created`（这正是 §2.2 的判据）。
- 全库回到审核前：`proposals=530`、`proposal_items=1738`、`concept_groups=284`。
- 遗留的 `anspire_excl_r115`（v2 算法 9 组 / v3 旧脏声明 8 组 / v4 修好 8 组）**不是本次审核创建**，本轮未删（v3 是 §9.1 的证据样本，建议 BA 决定是否清掉这个项目）。
- 未改 feature 分支、未 push；两个 QA worktree 与 `qa/REQ-115-2` 分支已删除。
