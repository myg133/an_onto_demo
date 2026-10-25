# QA 审核：REQ-144 · access mapping 阶段 0（契约 + validate + 离线 fixture + publish）

> Pre-merge 审核。审核人：QA Agent。审核时间：2026-10-25。
> 判据来源：`BA/demands/REQ-144/demand.md`（含 §三 设计修订 / §四 五产物 / §五 与 145 边界）+
> `acceptance.md`（9 条 AC + §三 三条裁定）+ `BA/design/2026-10-25-access-mapping-contract.md` +
> `BA/design/2026-10-25-interaction-contract.md` §3.3。
> **一律以自己实测为准**；Dev 自报的数字只当线索。

---

## 0 · 审核对象与环境

| 项 | 值 |
| --- | --- |
| 被测 | `feature-REQ-144` @ `b48e784`（分支 `feature/REQ-144`，单提交） |
| 基线 | `develop` `b4a8b56`（worktree `code/`） |
| 改动面 | **18 个文件**，`+10099 / −10`：新增 `src/access/{mod,fetch,validate,publish,path}.rs` + `fixtures/`（7 份）+ `tests/test_req144_access.rs`；改 `src/cli.rs`(+304)、`src/domain/pipeline.rs`(+78/−5)、`src/lib.rs`(+7)、两个 REQ-107 登记表 |
| 自建二进制（特性） | `.local/cargo-qa144-1/debug/ontok.exe` ⇒ sha256 `9665EA82EFACC91F55F73057A2CF91E1E2FDC64CB9E60915FCAE9C393398B0F5`（25 263 616 B） |
| 工具链 | cargo 1.96.0 / rustc 1.96.0（debug profile） |
| 元数据库 | `postgresql://postgres:postgres@172.17.0.87:5432/anspire_real`（**只读**用了一次：`ontology export` 对账，⛔ 未写库） |
| SPARQL 引擎 | rdflib **7.6.0** / Python 3.12.10 |
| target 目录隔离 | 特性 `.local/cargo-qa144-1`、基线 `code/.local/cargo-qa144-2`、变异副本 `.local/cargo-qa144-3`（三个互不共用；REQ-143 QA 记的「共用 target ⇒ 假特性二进制」坑已避开） |
| `access` 与库 | 三条子命令**在连库前分派**，全程未设 `ONTOK_DATABASE_URL`/`ONTOK_TEST_DATABASE_URL` 也能跑（实测） |

---

## 1 · 结论

> ## **FAIL** —— **1 条阻断（B-1，一行文档围栏）**；功能面 **9 条 P0 全部实证通过**、四条检查**全都有牙**、四条变异**全部变红**。

| # | 审核项 | 判 |
| --- | --- | --- |
| 1 | 四条 `validate` 检查真有牙（+ ④ 变异） | ✅ 四条各造坏一次全报错；**M1（④ 永远通过）⇒ 单测 + AC-002 双红** |
| 2 | 分页 + 本地过滤对照语义（+ 变异） | ✅ local_only ⇒ `rows_scanned`=250/`filtered_locally`=[status]；pushdown ⇒ 125/`[]`；**M2（两侧对调）⇒ 5 条红** |
| 3 | `max_scan_rows` 在**取页之前**拒绝 + stdout 空 | ✅ `已扫 100 行（1 页）`、stdout **0 字节**；**M4（拉完再判）⇒ `已扫 250 行` 变红** |
| 4 | R6 未签拒绝（逐条语义 + 变异） | ✅ 未签拒、同契约已签 entity 照常取、签上即通；**M3（去掉 R6）⇒ 双红** |
| 5 | 合成 fixture 的诚实性 | ✅ README 逐项登记「没有取过」；无「真实响应 / 已验证」字样；N-1/N-2 两条非阻断 |
| 6 | `--dry-run-fixture` 真的不联网 | ✅ `.invalid` 端点照跑；`src/access/**` 零 HTTP 调用；不给开关 ⇒ `DRY_RUN_REQUIRED` |
| 7 | `publish` 的 JSON-LD **真能查** | ✅ rdflib 解析 125 三元组；`fixtures/access-queries.rq` **原样**返回「cvm_instance 支持 zone 下推」 |
| 8 | 与 REQ-145 的边界 | ✅ `src/query/**`、`src/sparql/**` **零 diff**（只 import 了两个常量） |
| 9 | 不退化 / 零依赖 / 出口 66 | ✅ 四红线 + `tools.rs`(=`guard()`) + `property_filter.rs` + `Cargo.*` **零 diff**；出口 63→66（I29–I31）；❌ **唯一新增红 = `--doc`（B-1）** |

**B-1（阻断）**：`src/access/mod.rs:23` 的代码围栏写成了裸 ```` ``` ````（应为 ```` ```text ````），
rustdoc 于是把那段中文注释当 **Rust 代码**编译 ⇒ `cargo test --doc` **新增 1 条红**：

```
test src\access\mod.rs - access (line 23) ... FAILED
error: unknown start of token: \u{2190}      ← 「←」
24 | access.json      ← 作者格式：人写、人审、好 diff（本模块的**唯一输入**）
```

开发侧自己的判据没覆盖它：`tests/test_req144_access.rs` 走的是 `--test`，**不跑 doctest**。
基线 `b4a8b56` 的 doctest 是 **4 passed / 0 failed**；本分支 **4 passed / 1 failed** ⇒ 这是
「不退化」（AC-009）在这一轮**唯一**的破口。

修复量：**第 23 行加一个字 `text`**。我已在变异副本上验证过：只改这一处 ⇒ doctest 回到 **4 passed / 0 failed**（与基线逐条相同）。改完只需复跑 `cargo test --doc`（1.4 s），不必全量重跑。

> 定级说明（把判断摊开给 BA）：仓库里**没有** CI 配置（无 `.github/` / `Makefile`），所以「谁来跑 doctest」取决于
> 习惯——但只要有人跑 `cargo test`（含我这次的全量对比），就会看到这一条 FAILED。修复成本 = 1 个字，
> 我按「新增红一律不放过」定为**阻断**；若 BA 认为本项目不跑 doctest，可降级为非阻断，判据只有这一条。

---

## 2 · 四条 `validate` 检查是否真有牙（本需求的核心）

**做法**：把 `fixtures/` 拷到 `.local/qa144/fx/`，在副本上**各造一处坏**，跑
`ontok access validate --mapping <坏样例> --ontology access-ontology.ttl`，读 stderr / 退出码。
基线（未改坏）先跑一遍确认是绿的：

```
✓ access 契约自洽：2 条 entity / 1 条关系，四条检查全过（0 条提醒）
```

| # | 造坏方式 | 退出码 | 报出的码 + 点名的位置 | 报错可行动吗 |
| --- | --- | --- | --- | --- |
| ① | `"ontok:cvm_instance/{id}"` → `"ontok:云主机/{id}"` | 1 | `IRI_TEMPLATE_MISMATCH` `entities[0]（id=cvm_instance）字段 iri_template` | ✅ 两边字符串**逐字摆出**（`access.json : …#云主机/{id}` vs `本体 TTL : …#cvm_instance/{id}`）+ 指向「前缀/类名段要逐字一致」 |
| ② | `local_only` 里塞进已在 `pushdown` 的 `zone` | 1 | `CAPABILITY_OVERLAP` `entities[0]（id=cvm_instance）字段 capability` | ✅ 点名冲突字段 `["zone"]`，并说明「按 pushdown 读会静默丢掉本地过滤」 |
| ③ | 关系 `"to": "cvm_instance"` → `"cvm_disk"` | 1 | `RELATION_ENDPOINT_UNKNOWN` `relations[0]（predicate=alarm__cvm_instance）字段 to` | ✅ 点名 `cvm_disk`，并给出「v1 不支持跨源 join ⇒ 应标 `cross_source`」的出口 |
| ④ | `fields.zone.path` `"Placement.Zone"` → `"Placement.ZoneName"` | 1 | `FIELD_PATH_MISSING` `entities[0]（id=cvm_instance）字段 fields.zone.path` | ✅ `路径 … 在 fixture 的 250 行里一次都没出现 —— 映射写歪了`（并说明 `""`/`null` 算存在、这里是键根本不在） |

四条**都是一次只坏一处**、报错**只报该条**（基线其它三条仍绿），说明牙是各自独立的。

### 2.1 · ④ 的口径：是「至少一行存在」，不是「随便挑一行」

我自己另造一份 3 行夹具（`Placement.Zone` 只在 **1/3** 行里存在，`InstanceName` 有一行是 `null`）：

```
· entities[0]（id=cvm_instance）：字段 `zone` 的路径 `Placement.Zone` 只在 1/3 行里存在 ——
  缺失的行会**没有这个属性**（R2RML 口径：NULL 不产生三元组），确认这是有意为之。
✓ access 契约自洽：1 条 entity / 0 条关系，四条检查全过（1 条提醒）
  field_coverage = {"name":"3/3","zone":"1/3"}
```

⇒ 口径**正确**：部分缺失**不判红**（云 API 合法可空字段不该把整份映射弄红），但**报覆盖率 + 点名**；
`null` 计为「键在」；只有**全 0 命中**才 `FIELD_PATH_MISSING`。**不存在「只查第一行」的实现**
（`validate.rs:490–518` 对每一行都数命中数）。

### 2.2 · 变异 M1：把 ④ 改成「永远通过」

在源码副本里把 `if hit == 0` 改成 `if false && hit == 0`（等价于这条检查失效）：

```
lib   access::validate::tests::check4_field_path_missing  FAILED  (assertion failed: !r.ok)
集成  ac002_four_checks_have_teeth  FAILED
      panicked at tests\test_req144_access.rs:184: 路径在 fixture 里不存在必须红
```

⇒ **必须红，红了**（单测 + 集成判据双保险）。变异还原后 `git archive` 全树与 HEAD 逐字节相同，测试复绿（34 passed / 9 passed）。

---

## 3 · 合成 fixture 的诚实性（BA 裁定 §三）

### 3.1 · 逐项登记（要求 1）—— ✅ 合格

`fixtures/README.md:17–32` 有一张**逐项表**，且「什么时候取的」一栏写的是：

> `**没有取过** —— 这是合成夹具，不是脱敏真实响应`

「脱敏做了什么」写「不需要：ID 是 `ins-0001..ins-0250`、名称是 `web-0001..`，账号 / UIN / 密钥一个都没有」。
另有「诚实登记」段明说 `validate` 第 4 条**只证明「映射里的路径在这份夹具里存在」，⛔ 不证明在真实响应里存在**，
并要求阶段 1 拿到客户真响应后**整份替换**（要求 2 也写了）。

### 3.2 · 是否在别处写成「真实响应 / 已验证」（要求 3）—— ✅ 没有

对 `fixtures/**`、`src/access/**`、`src/cli.rs`、`tests/test_req144_access.rs` 全文搜
`真实响应 / 脱敏 / 已验证 / 验证过 / 现场数据 / 真实 API`：命中项全部是**否定句**或**限定句**
（例：`fetch.rs:62` 「另给 `freshness.source = "fixture"` 明说『这不是现场数据』」；
`fetch.rs` 的下推警告写「**真实调用时这一步由服务端完成**」——是在解释阶段 0 的边界，不是声称验过）。
运行期回执也带 `freshness.source = "fixture"`（实测每次都在）。

**唯一一处「真实」断言我单独证伪过**：`README.md:45` / `access-ontology.ttl:9` 说「真实导出
（`ontok ontology export`）里 `ontok:instanceIri` 长这样（**逐字节摘录**）」。我用真实库跑了一遍：

```
ontok ontology export --project anspire_r2rml_excl --version 2  →  83 个类
    ontok:instanceIri "https://ontok.local/anspire_r2rml_excl#{class}/{id}" .
```

⇒ **这条断言是真的**（不是文学修辞）。顺带把 AC-007 的「换真实本体会怎样」也验了：拿这份 83 类的
真实 TTL + 真实类名 `API_调用明细` 跑 `validate` ⇒ `ok: true`，两边字符串逐字相同：

```
iri_template = "https://ontok.local/anspire_r2rml_excl#API_调用明细/{id}"
本体 TTL 原值 = "https://ontok.local/anspire_r2rml_excl#{class}/{id}" ⇒ 展开 = 同左
```

### 3.3 · 非阻断 N-1：夹具与 `gen_access_fixtures.py` **不同源**

README 说脚本「生成上面两份响应…（确定性：**同一份代码 ⇒ 同一份字节**）」，脚本 docstring 还说
「夹具本身保持**纯响应形状**，⛔ 不夹注释字段」。实测**不成立**：

| 文件 | committed | 脚本重新生成 | 差异 |
| --- | --- | --- | --- |
| `tencent-cvm-describe.json` | 134 247 B | 124 022 B | committed 顶部多一个 **`_comment` 数组** |
| `tencent-monitor-alarm-histories.json` | 1 257 B | 1 017 B | 同上 |

即：committed 夹具**不是**这份脚本的产物（跑一次脚本会**删掉** `_comment`），所以「确定性」这条承诺目前拿不到；
另外 `_comment` 是**真实 DescribeInstances 响应里没有的键**（与「纯响应形状」相反）。功能上无影响
（`iterator`/路径全部落在 `$.Response.*` 里，实测 validate/fetch 全绿），但它让「阶段 1 整份替换」时多一处
需要人判断的东西。

### 3.4 · 非阻断 N-2：`_comment` 里有一条**与数据相反**的事实陈述

`tencent-cvm-describe.json:7` 写：

> `Zone 交替：奇数行 ap-guangzhou-3（125 行）、偶数行 ap-guangzhou-2（125 行）。`

实测（按行序号 1-based）：

```
rows 250 / TotalCount 250 / zone3 125 / zone2 125 / STOPPED 83
奇数行：zone3 = 0，zone2 = 125      ← 与注释相反
首行 ins-0001 的 Zone = ap-guangzhou-2
```

总数（各 125）与 STOPPED=83 都对，**只有奇偶对应关系写反了**。一个以「发现契约写错」为价值的 REQ，
自己的夹具注释写反，值得改一行字（不阻断）。

---

## 4 · `fetch --dry-run-fixture` 真的不联网

| 构造 | 结果 |
| --- | --- |
| `base_url` 改成 `https://cvm.invalid` / `https://monitor.invalid`（`.invalid` 是保留 TLD，**必然解析失败**） | ✅ `exit=0`，正常返回 5 个节点、`truncated:true` |
| 源码扫描：`src/access/**` 里 `ureq / reqwest / TcpStream / std::net` | ✅ **零命中**（只有注释与测试夹具里的 URL 字符串） |
| 不给 `--dry-run-fixture` | ✅ `exit=1`，`DRY_RUN_REQUIRED · 阶段 0 **不发任何网络请求**：请显式加 --dry-run-fixture 用夹具跑。…本程序里**没有**那条路径。`，**stdout 0 字节** |

⇒ 「不会偷偷联网」和「不会偷偷把夹具冒充真数据」两头都堵住了（`CODE_DRY_RUN_REQUIRED` 是显式拒绝，不是降级）。

---

## 5 · 分页 + 本地过滤的**对照语义**（最容易被做糊的地方）

同一份 250 行夹具、同一份契约（`max_limit=100`），只改过滤字段落在哪一侧：

| 场景 | `rows_scanned` | `pages` | `filtered_locally` | `pushdown_applied` | `capability_warnings` | 判 |
| --- | --- | --- | --- | --- | --- | --- |
| 无过滤（`--limit 1000`） | **250** | **3** | `[]` | `[]` | 空 | ✅ 250/100 ⇒ 3 页 |
| `{"status":"STOPPED"}`（`status` ∈ **local_only**） | **250**（全量） | 3 | **`["status"]`** | `[]` | 「字段 `status` 不在 `pushdown` 里（服务端不支持按它筛）⇒ 已在本地过滤」 | ✅ 三项全对 |
| `{"zone":"ap-guangzhou-3"}`（`zone` ∈ **pushdown**） | **125**（变小） | 2 | **`[]`** | `["zone"]` | 空 | ✅ 下推**不**计入本地过滤 |
| `{"name":"web-0001"}`（在 `fields` 里、但两侧都没声明） | 250 | 3 | `["name"]` | `[]` | 「既不在 pushdown 也不在 local_only —— 按**本地过滤**处理…建议显式声明」 | ✅ 保守，且说了 |
| `{"region":"ap-guangzhou"}`（声明下推、但响应里**没有**这个字段） | **100**（第 1 页就够 `--limit 5`；**未**按 region 筛） | 1 | `[]` | `[]` | 「声明为下推，但 fixture 里没有能对上它的路径 ⇒ **模拟不了**…**未**按 region 过滤」 | ✅ 不假装筛过 |

补充两条语义（都实测过，与需求一致）：

- **`--limit` 会提前停**：`status=STOPPED` + `--limit 50` ⇒ `rows_scanned=200`、`pages=2`、`rows_matched=66`、
  `rows_returned=50`、`truncated=true`。即「local_only ⇒ `rows_scanned` = 全量」的**准确表述**是
  「在 `--limit` 允许的范围内扫到能确定结果为止」：`--limit 200` ⇒ 250 全扫；`--limit 50` ⇒ 扫到第 200 行就够。
  这是 `--limit` 的语义而非 bug，但写验收/文档时别把「全量」写成无条件。
- **`meta` 三项齐全**（AC-008）：`filtered_locally` / `freshness`（含 `mode` / `note` / `fetched_at` / `source:"fixture"`）/
  `capability_warnings` 每次都在；另带 `mapping_ref: "access#cvm_instance@v2"`（与交互契约 §3.3 的
  `access#<entity.id>@v<mapping_version>` 同一拼法）、`unsimulated_pushdown`、`pushdown_applied`。

### 5.1 · 变异 M2：把 pushdown / local_only **对调**

```
lib   access::fetch::tests::{pages_and_local_filter, pushdown_narrows_server_side,
                            unsimulated_pushdown_is_reported_not_faked}  FAILED
集成  ac004_paging_and_local_filter   FAILED  (left: [] / right: ["status"])
      ac008_meta_carries_the_five_fields  FAILED
```

⇒ **必须红，红了**（5 条）。变异还原后逐字节相同、复绿。

---

## 6 · `max_scan_rows` 拒绝在**取页之前**（R5）

契约副本把 `cvm_instance.max_scan_rows` 从 5000 改成 **120**（250 行 / 每页 100）：

```
exit=1，stdout = 0 字节
✗ SCAN_LIMIT_EXCEEDED · 需要继续扫描就会超过 `max_scan_rows`：已扫 100 行（1 页），
  再取一页要 100 行 —— 合计会到 200，预算只有 120。
  ⛔ 拒绝返回部分结果。请**改查询**（加更窄的过滤 / 调小 --limit），⛔ 不要重试 —— 重试只会再撞同一面墙。
```

| 判据 | 实测 |
| --- | --- |
| 超预算 ⇒ `SCAN_LIMIT_EXCEEDED` | ✅ |
| ⛔ **stdout 必须为空** | ✅ **0 字节**（`(Get-Item …).Length == 0`） |
| ⛔ 不许「拉完再判」 | ✅ 报错说「已扫 **100** 行（**1** 页）」——第 2 页**没取** |
| 报错可行动（改查询 / 别重试） | ✅ 且与 `RATE_LIMITED` 分开了（这条不进「可重试」类） |

### 6.1 · 变异 M4：「拉完再判」（把判定挪到循环之后）

```
lib   access::fetch::tests::scan_limit_is_refused_before_fetching  FAILED
      报错要说清读了多少行…：超过 `max_scan_rows`：已扫 12 行（3 页），预算只有 7
集成  ac005_scan_limit_is_refused_without_partial_result  FAILED
      超过 `max_scan_rows`：已扫 250 行（3 页），预算只有 120
```

⇒ 「取页前 / 取页后」这个差别是**可观测的**（100 vs 250），判据钉住了它。

---

## 7 · R6 未签拒绝（逐条语义）

| 构造 | 结果 |
| --- | --- |
| 只把 `cvm_instance` 的 `reviewed_by` 改 `null`（`alarm` 仍签） | ✅ `exit=1` `UNREVIEWED_MAPPING · entity cvm_instance 的 reviewed_by 为空 —— 未签的映射不许取数（R6 / ADR-005）…`，**stdout 0 字节** |
| **同一份契约**里取已签的 `alarm` | ✅ `exit=0`，扫 3 行 / 返回 3 节点，`mapping_ref: access#alarm@v2` |
| 把 `reviewed_by` 签回去再取 | ✅ `exit=0`，正常返回 |

⇒ R6 是**逐条 entity** 的闸门（不是「整份契约作废」），且不是「永远不行」——两条语义都验了。

### 7.1 · 变异 M3：去掉 R6 闸门

```
lib   access::fetch::tests::unreviewed_is_rejected  FAILED
      未签的映射居然取到数了：扫 5 行、返回 5 行
集成  ac006_unreviewed_entity_is_refused  FAILED
      未签的映射居然取到数了：扫 100 行、返回 5 行
```

⇒ **必须红，红了**。

---

## 8 · `publish` 的 JSON-LD **真能查**（不是口号）

自己跑，不采信「用 rdflib 实测过，见交付报告」这句自述：

```
ontok access publish --mapping fixtures/access-cvm.json --ontology fixtures/access-ontology.ttl \
                     --out .local/qa144/access.jsonld        →  7 093 B，顶层只有 @context + @graph

python + rdflib 7.6.0：
  g.parse(access.jsonld, format="json-ld")   →  125 triples
  g.query(open('fixtures/access-queries.rq').read())  →  1 行
      ['https://ontok.local/ops360#cvm_instance', 'DescribeInstances', '5000', 'fde@ops360 (2026-10-25)']
```

**`fixtures/access-queries.rq` 原样（未改一个字）** 就跑出了需求点名的那一问：「**哪些类支持按 zone 过滤**」
⇒ `cvm_instance` / `DescribeInstances` / `maxScanRows=5000` / 签字人。另外我又自己写了 5 条（都在同一份图上跑通）：

| 自写查询 | 命中 |
| --- | --- |
| Q2 `ontok:localOnly` | `alarm/status`、`cvm_instance/cpu`、`cvm_instance/status` |
| Q3 `ontok:reviewedBy` | `alarm`、`cvm_instance` 各 1 行 |
| Q4 关系边（含 `crossSource` / `traversable`） | `alarm__cvm_instance` → `traversable=false` |
| Q5 RML 核心（`rml:TriplesMap` → `rr:logicalSource/rml:iterator` + `rr:subjectMap/rr:template` + `rr:class`） | 两条 TriplesMap，`rr:class` 是 **IRI 节点**（不是字符串） |
| Q6 字段路径（`rr:predicateObjectMap/rr:objectMap/rml:reference`） | 8 条，`InstanceName` / `Placement.Zone` / `CPU` … 全部可按类查 |

⇒ 「映射可查询」**不是口号**。附带验了未签映射也能发布并被标准处理器解析（124 triples，`reviewedBy` 的
`null` 被 JSON-LD 丢弃 ⇒ 见 N-4）。

---

## 9 · 与 REQ-145 的边界

```
$ git diff develop..HEAD --name-only
fixtures/README.md                       src/access/fetch.rs
fixtures/access-cvm.json                 src/access/mod.rs
fixtures/access-ontology.ttl             src/access/path.rs
fixtures/access-queries.rq               src/access/publish.rs
fixtures/gen_access_fixtures.py          src/access/validate.rs
fixtures/tencent-cvm-describe.json       src/cli.rs
fixtures/tencent-monitor-alarm-histories.json  src/domain/pipeline.rs
                                         src/lib.rs
tests/test_req107_outlet_pipeline.rs      tests/test_req107_source_enumeration.rs
tests/test_req144_access.rs
```

```
$ git diff develop..HEAD --name-only -- src/query/** src/sparql/**   →   （空）ZERO DIFF
```

- ⛔ **没有** `src/query/*`、**没有** `src/sparql/*`、**没有** `query serve` / `query/http.rs` 的改动 ⇒ 边界守住。
- 对 `query` 的**唯一**接触是一条**只读 import**：`src/access/fetch.rs:80`
  `use crate::query::jsonld::{ONTOK_PREFIX, XSD_NS};`（复用既有常量，避免抄第二份前缀）——不是改动。

---

## 10 · 不退化 / 零依赖 / 出口登记（数字全部本机实测）

### 10.1 · 零 diff（逐 blob 核对，不是只看文件名）

| 项 | 结果 |
| --- | --- |
| 四红线 `tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs` / `src/domain/review_view.rs` | ✅ `f3967ecd` / `6c0ae562` / `9d085a03` / `fdd7fcd9` —— 与 `develop` **逐一相同** |
| `src/mcp/tools.rs`（`pub fn guard()` 在 `tools.rs:693`，同文件） | ✅ ZERO DIFF |
| `src/domain/property_filter.rs` | ✅ ZERO DIFF |
| `Cargo.toml` / `Cargo.lock` | ✅ ZERO DIFF ⇒ **零新增依赖**（阶段 0 不发网络，`ureq` 一行未用） |

### 10.2 · 出口登记 I29 / I30 / I31 ⇒ 63 → 66

`src/domain/pipeline.rs`：新增 `I29`（`validate` 报告 → stdout）、`I30`（`fetch` JSON-LD → stdout）、
`I31`（`publish` → `--out`），三条**全部内部轨**；`registry_is_the_declared_baseline` 断言
**66（内部 36）**，并同步改了两个 REQ-107 登记表（`MODULES` / `OUTLET_FNS` / `I{i}` 1..=31）。
实测：`test_req107_outlet_pipeline` **12 passed**、`test_req107_source_enumeration` **10 passed**、
`test_req107_red_lines` **2 passed**、`test_req107_track_manifest` **2 passed**。

### 10.3 · 基线数字（我在同一台机、各自独立 target 目录下各跑一次全量 `cargo test --no-fail-fast`）

| | 基线 `develop b4a8b56` | 特性 `b48e784` | 差 |
| --- | --- | --- | --- |
| 测试目标数 | 100 | **101**（+`test_req144_access`） | +1 |
| 通过 / 失败 | **1579 / 183** | **1622 / 184** | +43 / **+1** |
| `--lib` | 743 passed / 2 failed | **777 passed / 2 failed** | **+34**（`access::` 34 条），失败数不变 |
| `test_req144_access` | — | **9 passed / 0 failed** | 新增 |
| `--doc` | **4 passed / 0 failed** | **4 passed / 1 failed** | ❌ **B-1** |

> **环境说明（重要，避免误读那 183 个红）**：本会话没有 `ONTOK_TEST_DATABASE_URL`，凡是要真库的目标
> 一律以 `MissingEnv("ONTOK_TEST_DATABASE_URL")` 失败。**基线与特性两边红的集合与条数完全相同**
> （30 个目标、同名同位，逐条对拍：`test_snapshot*` / `test_review_show*` / `test_mcp_server` /
> `test_apply` / `test_project` / `lib` 的 2 条 … 连 passed 数都一样）。⇒ 这些是**环境前置**，不是本分支引入。
> 唯一**只在特性侧新增的红**是 `--doc`（B-1）。我没有为了"刷绿"去往共享库写测试数据（只做了一次
> 只读 `ontology export` 用于 §3 的诚实性对账）。

---

## 11 · 非阻断发现（建议合并前顺手改，不卡功能）

| # | 发现 | 证据 | 建议 |
| --- | --- | --- | --- |
| **N-1** | 夹具与 `gen_access_fixtures.py` **不同源**；README「确定性：同一份代码 ⇒ 同一份字节」不成立；committed 夹具顶部多一个 `_comment`（脚本反而**不**生成它，与脚本 docstring「⛔ 不夹注释字段」相反） | sha256：committed `F33F48…F6C0` / 重生成 `FEB22D…2933`；134 247 B vs 124 022 B（alarm：`89191A…DB43` vs `C58A18…A8D`） | 二选一：①把 `_comment` 写进生成脚本（恢复「同一份字节」）；②把 README 的「确定性」改成「形状参考，`_comment` 为手工标注」 |
| **N-2** | `tencent-cvm-describe.json` 的 `_comment` 把 zone 的奇偶写反了（实为奇数行 = `ap-guangzhou-2`） | 奇数行 zone3 = **0**、zone2 = **125**；首行 `ins-0001` = `ap-guangzhou-2` | 改一行字（各 125 的总数没错，只有奇偶对应反了） |
| **N-3** | `fixtures/README.md:14` 说 `access-queries.rq`「证明映射可查（**AC-009**）」；`acceptance.md` 的 AC-009 是「不退化 / 零依赖」（可查性判据在这份报告 §8 / `test_req144_access.rs` 的 `ac009_*` 命名里） | 两份文件对照 | 改成「见 `tests/test_req144_access.rs::ac009_published_jsonld_is_queryable` + QA 报告 §8」 |
| **N-4** | `publish.rs:198` 注释说未签的 `reviewedBy` 写 `null`「⛔ 不省略：省略与『签了空串』在查询里分不开」，但标准 JSON-LD 处理器会**丢弃 null** ⇒ 图里该三元组**缺失** | rdflib 解析未签发布物：124 triples，`objects(None, ontok:reviewedBy)` 只剩已签那一条 | 注释改成事实（「未签 ⇒ 图中无此三元组，用 `FILTER NOT EXISTS` 查未签」），或改成显式哨兵值 |
| **N-5** | （流程项，属 dispatch 不属 Dev）`.feature/manifest.json` 仍写着 **REQ-123** 的分配信息，本 worktree 未更新 | `req_id: "REQ-123"` | BA 合入前顺手更新（避免下次找不到「这个 worktree 是谁的」） |

---

## 12 · 我的临时目录与清理

全部在 worktree 的 `.local/` 下（⛔ 未在 `D:\` 根建目录、⛔ 未改 feature 分支、⛔ 未 push）：

| 路径 | 是什么 | 大小 | 处置 |
| --- | --- | --- | --- |
| `feature-REQ-144/.local/cargo-qa144-1` | 特性分支独立 target（含 `ontok.exe` 与测试可执行） | 16.3 GB | **已删除**（需要复验时重建 ≈4 min） |
| `feature-REQ-144/.local/cargo-qa144-3` | 变异副本 target（M1–M4） | 17.6 GB | **已删除** |
| `code/.local/cargo-qa144-2` | 基线 `develop` 独立 target（基线数字的来源） | 15.9 GB | **已删除** |
| `feature-REQ-144/.local/qa144/` | 证据：四份坏样例、`c1–c8` 变异映射、`access.jsonld`、`query_check.py`、`real-ttl.ttl`、各次 `*.out/*.err` 与全量测试日志 | 8.6 MB | **保留**（复现本报告任何一行都够） |
| `feature-REQ-144/.local/qa144/mut/` | `git archive HEAD` 出来的变异副本（已还原为与 HEAD 逐字节相同） | — | 保留 |
| `code/.local/qa144-base/` | 基线全量测试日志 | 0.4 MB | 保留 |
| `feature-REQ-144/.local/fixtures/` | 跑 `gen_access_fixtures.py` 的产物（N-1 的证据） | 0.1 MB | 保留 |

> 删除的三个 target 目录是我自己建的（`qa144` 前缀），**没有**碰 Dev 的 `.local/cargo-req144`。
> 变异全部在 `git archive` 副本里做，**feature 分支工作树未被写入一个字节**（`git status` 干净）。

---

## 13 · 复核建议（修完 B-1 后）

只需两步（合计 < 1 分钟，不必全量重跑）：

```bash
cargo test --doc                                  # 期望 4 passed / 0 failed（与基线相同）
cargo test --lib access:: ; cargo test --test test_req144_access   # 期望 34 / 9，全绿
```

功能面我不建议再动：四条检查的牙、对照语义、R5 的「取页前」、R6 的逐条语义、SPARQL 可查性
这五处，本轮都是**自己造坏 → 必须红 → 确实红**验过的。

---

## 14 · 复验（第二轮）：B-1 + N-1 ~ N-4 —— **PASS**

> 复验对象：`feature/REQ-144` @ **`c01c70b`**（`b48e784` 功能 + `c01c70b` 修复），基线 `develop` `b4a8b56`。
> 复验时间：2026-10-25。**只验上轮那 5 条 + 两条确认**（功能面 9 条 P0 语义按任务书**不重跑**，上轮已实证）。
> 起跑前 `git status` 空、HEAD = `c01c70b`；收尾后仍空、未新增提交、**未 push**、未改 feature 分支。

### 14.1 · 结论

**PASS** —— 5 条全部转绿；修复提交对 `src/` 只动 **2 处、+11/−2 行，全在注释/围栏里**，功能面 34/9 与上轮逐条同名。

| # | 上轮判定 | 本轮实测 | 判 |
| --- | --- | --- | --- |
| **B-1** | doctest 4 passed / **1 failed** | 自己跑 ⇒ **4 passed / 0 failed**，四条与基线**同名同序** | ✅ 关 |
| **N-1** | 夹具 ≠ 脚本产物（sha 不同、committed 多出 `_comment`） | 重跑脚本两次 ⇒ sha256 **等于库里的版本**；`git status` **空** | ✅ 关 |
| **N-2** | `_comment` 把 zone 奇偶写反 | 脚本**读回夹具**算分布 ⇒ `_comment` 五句**逐条一致** | ✅ 关 |
| **N-3** | README 把「可查性」冒充 AC-009 | README + 测试名均改称「额外自证（⛔ 不是 AC-009）」；AC-009 由既有判据覆盖且**本轮实测全绿** | ✅ 关 |
| **N-4** | 注释说 null「不省略」，实际 JSON-LD 丢弃 null | rdflib **自己复现**：2 条映射、1 条未签 ⇒ 图里**只有 1 条** `ontok:reviewedBy`；注释已改成实际行为 | ✅ 关 |

### 14.2 · B-1 · doctest 回 4/0（自己跑，不采信 Dev）

```text
$ CARGO_TARGET_DIR=.local/cargo-rqa144-1 cargo test --doc
   Doc-tests ontok
running 4 tests
test src\agent\llm\mod.rs - agent::llm (line 100) - compile ... ok
test src\agent\acp\mod.rs - agent::acp (line 203) - compile ... ok
test src\domain\pipeline.rs - domain::pipeline (line 37) - compile fail ... ok
test src\domain\pipeline.rs - domain::pipeline (line 49) - compile fail ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s
```

与基线对拍（`code/.local/qa144-base/full-test-base.log:5195`）：基线同样是这 **4 条、同名同序、4/0** ⇒ 「回到基线」，不是靠删测试凑数。
修复方式核对：`git diff b48e784 c01c70b -- src/access/mod.rs` 只有 **1 行** —— `//! ``` ` → `//! ```text`，而那段围栏里是
`access.json ← 作者格式 / access.jsonld ← 发布格式` 的**中文说明图**（不是 Rust 代码）⇒ 本来就不该被 rustdoc 当代码编。

### 14.3 · N-1 · fixture 与生成脚本同源 + 幂等

工作树 `fixtures/`，`python fixtures/gen_access_fixtures.py` 连跑两次（全程 `git status` 空、未提交）：

| 时点 | `tencent-cvm-describe.json` sha256 / git blob | `tencent-monitor-alarm-histories.json` sha256 / git blob |
| --- | --- | --- |
| 重跑前（= 库里 `c01c70b` 版本） | `62a4cc99…486857` / `0e49b1d8…1729` | `7eda3a1c…a1af4` / `6156e667…9fb6` |
| 第 1 次重跑后 | `62a4cc99…486857` / 同上 | `7eda3a1c…a1af4` / 同上 |
| 第 2 次重跑后（幂等） | `62a4cc99…486857` / 同上 | `7eda3a1c…a1af4` / 同上 |
| `git status --porcelain` | **空** | **空** |

结论：①**库里的就是脚本产物**（sha256 与 git blob 三重相等）；②**幂等**（两次连跑逐字节相同）；
③`_comment` 现在由脚本自己写（`gen_access_fixtures.py:41-48 / 94-100`），上轮「脚本不产 `_comment`」的矛盾消失。
尺寸也回到「纯数据」量级：cvm **124 625 B**（上轮 committed 版 134 247 B，多出来的是手写注释 + 缩进差异）。

### 14.4 · N-2 · `_comment` 与数据逐条一致

判据做法：**不用脚本里的常量**，直接 `json.load` 夹具、按行号重算分布，再和 `_comment` 文字逐句比。
证据脚本：`.local/qa-rqa144-1/check_comment.py`（保留）。

| `_comment` 原句 | 脚本从夹具读回的实测 | 判 |
| --- | --- | --- |
| `250 行 / paging.max_limit=100 ⇒ 3 页（100/100/50）` | `len(InstanceSet)=250`、`TotalCount=250`、`max_limit=100` ⇒ 按 100 切得 **100/100/50** | ✅ |
| `InstanceState=STOPPED 83 行（i%3==0 ⇒ 3,6,…,249），其余 167 行 RUNNING` | STOPPED **83**（尾号 3/6/9/12/15/18…）、RUNNING **167**、其它状态 **0** | ✅ |
| `Zone 交替：奇数行 ap-guangzhou-2（125 行）、偶数行 ap-guangzhou-3（125 行）` | 奇数行 `{ap-guangzhou-2: 125}`、偶数行 `{ap-guangzhou-3: 125}`（各 125，**奇偶方向与文字一致**） | ✅ |
| `3 行；AlarmObject = 实例 ID（CVM 的告警对象就是实例）⇒ 关系 alarm__cvm_instance 的 join 列` | `len(AlarmHistorySet)=3`、`AlarmObject=[ins-0002, ins-0004, ins-0006]`（确实是实例 ID） | ✅ |
| `AlarmStatus：ALARM 2 行（alarm-0001/0003）、OK 1 行（alarm-0002）` | ALARM `[alarm-0001, alarm-0003]`、OK `[alarm-0002]` | ✅ |

`check_comment.py` 收尾打印 `N-2 结论: PASS（_comment 与实测逐条一致）`（exit 0）。
另：README §「两份响应夹具的口径」表里的同一组数字（250 行 / 83 STOPPED / 各 125 / 3 行 / 2 ALARM）与上表一致，**文档也没漂**。

### 14.5 · N-3 · 可查性不再是 AC-009，且 AC-009 仍被既有判据覆盖

**改后那行（`fixtures/README.md:14`）**：

```text
| `access-queries.rq` | 发布态 `access.jsonld` 的 SPARQL 查询示例 | 证明映射可查（**额外自证**，⛔ 不是 AC-009） |
```

**测试名（`tests/test_req144_access.rs`）**：

```text
:18  | [`extra_published_jsonld_is_queryable`] | **额外自证**（⛔ 不是 AC-009） | 发布物里 RML 核心 + `ontok:` 扩展都在，能按 pushdown 查 |
:20  ⚠️ **AC-009 是「不退化 / 零依赖」，不是「可查性」**（`acceptance.md` 原文）。
:402 fn extra_published_jsonld_is_queryable() {      ← 全仓已无 ac009_published_jsonld_is_queryable
```

**AC-009 判据的对应关系（`acceptance.md:55-62` 四项逐项落地，本轮实测）**：

| AC-009 原文约束 | 落到哪条既有判据 | 本轮实测 |
| --- | --- | --- |
| ⛔ 禁改：四红线 + `tools.rs` + `guard()` + `property_filter.rs` | `tests/test_req107_red_lines.rs` | **2 passed / 0 failed** |
| 出口：新子命令登记 `OUTLETS`（I29/I30/I31，共 66 条） | `tests/test_req107_source_enumeration.rs` + `tests/test_req107_outlet_pipeline.rs` + `tests/test_req107_track_manifest.rs` | **10 / 12 / 2 全绿** |
| ⛔ 新依赖（零依赖） | `git diff develop -- Cargo.toml` / `Cargo.lock` | **两边都为 0 行**（`[]`，逐字节零 diff） |
| 基线同机实测 | 见 §14.8 全量表 | 失败数 183 = 基线 183 |

⇒ 可查性从 AC-009 的位置上退下来，**AC-009 本身没有出现覆盖空洞**（上表四行都是既有测试，不是本轮新写的「自己出题自己答」）。

### 14.6 · N-4 · `reviewedBy: null` 注释 vs 实际行为（自己复现）

做法：拿 `fixtures/access-cvm.json` 复制一份，把 **`alarm` 的 `reviewed_by` 改成 `null`**（`cvm_instance` 保持已签），
`ontok access publish` 出 JSON-LD，再用 **rdflib 7.6.0** 解析数三元组。证据脚本：`.local/qa-rqa144-1/repro_reviewedby.py`。

```text
## 1) 发布态的原始 JSON（两条映射）
  urn:ontok:access:ops360#alarm          键在=True  值=None
  urn:ontok:access:ops360#cvm_instance   键在=True  值='fde@ops360 (2026-10-25)'

## 2) rdflib 解析后的三元组计数
  1 条未签（2 条映射）   全图三元组=124  ontok:reviewedBy 三元组=1
      cvm_instance  → 'fde@ops360 (2026-10-25)'
  2 条全签（2 条映射）   全图三元组=125  ontok:reviewedBy 三元组=2
      alarm → 'fde@ops360 (2026-10-25)'
      cvm_instance → 'fde@ops360 (2026-10-25)'

## 3) 注释推荐的查法（在「1 条未签」那张图上）
  FILTER NOT EXISTS  ⇒ 未签的映射: ['alarm']  （1 条）
  OPTIONAL+!BOUND    ⇒ 未签的映射: ['alarm']  （1 条）
  全量 OPTIONAL      ⇒ alarm reviewedBy=unbound ; cvm_instance reviewedBy='fde@…'
```

与注释（`src/access/publish.rs:198-207`）逐句对：

| 注释断言 | 实测 | 判 |
| --- | --- | --- |
| 作者格式里未签写 `null`（不是省略键） | 发布 JSON 里 `alarm` 的 `ontok:reviewedBy` 键**在**、值 `null`；`cvm_instance` 有值 | ✅ |
| 「行为上等于不写」：JSON-LD 丢弃 `null` ⇒ RDF 里没有该三元组 | 2 条映射 1 条未签 ⇒ **1 条**三元组（=Dev 自报的「2 条映射、1 条未签，图里 1 条」） | ✅ |
| 不是「一律不写」（反证） | 2 条全签 ⇒ **2 条**三元组 | ✅ |
| 未签者**不是**空串 / null 字面量 | 未签那条完全没有该三元组；全量 OPTIONAL 下它是 `unbound` | ✅ |
| 查未签必须用 `FILTER NOT EXISTS` 或 `OPTIONAL + FILTER(!BOUND(...))` | 两种查法**都恰好查出 `alarm` 1 条**，结果相同 | ✅ |
| ⛔ 不能查「值等于 null」（RDF 没有 null 字面量） | 全量 OPTIONAL 里未签者是 unbound，不是任何字面量 | ✅ |

### 14.7 · 6 · 功能面未被碰到（34 / 9 与上轮逐条同名）

| 命令 | 结果 |
| --- | --- |
| `cargo test --lib access::` | **34 passed / 0 failed**（745 filtered out） |
| `cargo test --test test_req144_access` | **9 passed / 0 failed**，9 条用例名与上轮**完全一致**（`ac001…ac008` + `extra_published_jsonld_is_queryable`） |

**diff 证据**（`git diff b48e784 c01c70b`，即「功能提交 → 修复提交」）：

| 文件 | 增/删 | 内容 |
| --- | --- | --- |
| `src/access/mod.rs` | +1 / −1 | 仅围栏 ` ``` ` → ` ```text ` |
| `src/access/publish.rs` | +10 / −1 | **仅注释**（`reviewedBy` 那段） |
| `fixtures/gen_access_fixtures.py` | +20 / −2 | `_comment` 改由脚本产出、zone 奇偶改对 |
| `fixtures/tencent-*.json` | +9 / −255 | 脚本重新生成（`_comment` 落盘、缩进归一） |
| `fixtures/README.md` | +14 / −4 | 文档 |
| `tests/test_req144_access.rs` | +15 / −4 | 仅文件头表格 + 用例**改名**（`ac009_*` → `extra_*`），断言体一行未动 |

⇒ `src/` 侧 **没有任何一条可执行语句变化**（+11/−2 行全部落在 `//!`/`//` 注释与围栏上）⇒ 上一轮验过的 9 条 P0 语义无需重跑。

### 14.8 · 7 · 全量 `cargo test --no-fail-fast`（裸跑，**含 doctest**）

命令：`CARGO_TARGET_DIR=.local/cargo-rqa144-1 cargo test --no-fail-fast`（**未**加 `--lib`/`--tests`；日志 `.local/qa-rqa144-1/full-test.log`）。
**未设 `ONTOK_TEST_DATABASE_URL`** —— 与上轮口径一致（⛔ 不往共享库写测试数据；`access` 三条子命令在连库前分派，不需要它）。

| | 基线 `develop b4a8b56` | 特性 `c01c70b` | 差 |
| --- | --- | --- | --- |
| 测试目标数 | 104 | **105** | +1（`test_req144_access`） |
| passed | 1627 | **1670** | **+43**（lib `access::` 34 + 集成 9） |
| failed | **183** | **183** | **0** |
| ignored | 2 | 2 | 0 |
| 失败目标数（`error: N targets failed`） | 29 | **29** | 0 |
| Doc-tests | 4 / 0 | **4 / 0** | 0 |

**失败逐条是否基线同红**：按目标对拍 `full-test.log` vs `code/.local/qa144-base/full-test-base.log`，
**104 个共有目标里没有一个 passed/failed 数字发生变化**（唯一差异是 lib `743→777 passed`、以及多出来的
`test_req144_access` 9 passed）；失败用例名集合 **只在两边同红 182 条、单边红 0 条**（另有 1 条 lib 用例名因其 stdout
被测试框架拼到同一行、两边的解析器都没抠出来，但**该目标 failed 数两边都是 2/2**，属同名同因）。
⇒ **本分支零新增红**，183 条全部是 `MissingEnv("ONTOK_TEST_DATABASE_URL")` 这类**环境前置**（与基线同名同位同因）。

**flaky 处置（`test_extraction_agent`）**：全量里它 **18 passed / 8 failed**，失败的是
`test_registry_*`（6 条）+ `test_cli_*registry*`（2 条），**失败原因是 `MissingEnv("ONTOK_TEST_DATABASE_URL")`**
（`tests/common/mod.rs:233` / `tests/test_extraction_agent.rs:1377,1421`），**不是**并行才会出现的断言型 flake。
单独复跑该二进制（`cargo test --test test_extraction_agent --no-fail-fast`）⇒ **18 passed / 8 failed，一字不差地复现**。
⇒ 判：**环境前置红，非本分支引入，也不是本轮需要处置的 flaky**（Dev 说的「单跑 26/26 绿」只有在设了
`ONTOK_TEST_DATABASE_URL` 时成立；我没设，以免往共享库写测试数据）。

### 14.9 · 9 · 我的临时目录与清理

全部在 worktree 的 `.local/` 下（⛔ 未在 `D:\` 根建目录、⛔ 未改 feature 分支、⛔ 未 push、⛔ 未碰 Dev 的 `.local/cargo-req144` 与上轮 QA 的 `.local/qa144`）：

| 路径 | 是什么 | 大小 | 处置 |
| --- | --- | --- | --- |
| `feature-REQ-144/.local/cargo-rqa144-1` | 本轮独立 `CARGO_TARGET_DIR`（doc / lib / 集成 / 全量共用） | 15.89 GB | **已删除**（需再复验时重建 ≈1–2 min） |
| `feature-REQ-144/.local/qa-rqa144-1` | 证据：`check_comment.py`、`repro_reviewedby.py`、`sum_tests.py`、`access-one-unsigned.{json,jsonld}`、`access-both-signed.jsonld`、`full-test.log` | 0.39 MB | **保留**（本报告 §14.3–§14.8 每一行都能复现） |

收尾复核：`git status` **空**、`git log -1` 仍是 `c01c70b`。

### 14.10 · 过程诚实登记

本轮我第一版判据有**两处自伤**（不是 Dev 的问题，记下来免得以后当成「一次就绿」）：

1. N-2 里 `_comment` 写的是简写 `alarm-0001/0003`，我的字符串比对按 `"/".join(ids)` 比 ⇒ 假红；改成比 4 位尾号集合后 PASS。
2. N-4 里我先按 `@type == "ontok:Mapping"` 查未签，但发布态实际的类型是 `rml:TriplesMap`（`@id` 前缀是
   `urn:ontok:access:ops360#`）⇒ 查询命中 0、假红；按实际类型/ID 重写后 PASS。

两次都是**先看到 FAIL、追到判据本身**才改的，没有为了让结果好看去改被测数据。

### 14.11 · 放行意见

**PASS，可进 PR。** 5 条全部关闭，功能面 diff 只落在文档/注释/夹具/测试名，全量与基线零新增红。
合并前顺手可做（**不阻断**，属 dispatch）：

- `.feature/manifest.json` 仍写着 REQ-123 的分配信息（上轮 N-5，本轮未修，属 BA/worktree 管理面）。
