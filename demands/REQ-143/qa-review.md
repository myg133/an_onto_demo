# QA 审核：REQ-143 · 纯文档通路（`doc merge --allow-doc-only`）

> Pre-merge 审核。审核人：QA Agent。审核时间：2026-10-25。
> 判据来源：`BA/demands/REQ-143/demand.md` + `acceptance.md`（含 §三 三条裁定 R-1 / R-2 / R-3）。
> 一律以**自己实测**为准；Dev 自报的数字只当线索。

---

## 0 · 审核对象与环境

| 项 | 值 |
| --- | --- |
| 被测 | `feature-REQ-143` @ `73e02d3`（分支 `feature/REQ-143`，单提交） |
| 基线 | `develop` `f04b2ae`（worktree `code/`） |
| 改动面 | **2 个文件**：`src/cli.rs` **+57 / −1**、`src/extract/doc_merge.rs` **+988 / −37** |
| 自建二进制（基线） | `cargo build --bin ontok` @ `code`/`f04b2ae` ⇒ sha256 `9b6d855c1457a39a3033503ba37aa20b4e9fdc1557dd5c9a749451f25e46fdaa`（`--help` 里**没有** `--allow-doc-only`，传该参数 ⇒ `exit 2`） |
| 自建二进制（特性） | 同上 @ `feature-REQ-143`/`73e02d3` ⇒ sha256 `1f96ba656897a506d0f850e7ca6fe82082de0779808b74cedf6f2eacee6ba3b4` |
| 工具链 | cargo 1.96.0 / rustc 1.96.0（debug profile） |
| 元数据库 | `postgresql://postgres:postgres@172.17.0.87:5432/anspire_real`；端到端项目名 **`req143_qa`**（⛔ 未碰 `ops360` / `anspire_r2rml_excl`） |
| 三组输入 | **c1** 3 断言（订单/客户 + 下单）+ 空提案 · **c2** 56 行 req116 fixture + `code/fixtures/proposal-demo.json` · **c3** 728 条真实断言（`.local/ops360/assertions.jsonl`）+ 30 条老提案（`.local/ops360/proposal.json`） |

### ⚠️ 复验前必读的一个坑（我自己踩了）

**两个 worktree 共用一个 `CARGO_TARGET_DIR` ⇒ cargo 会复用另一个 worktree 的产物。**
实测：在 `feature-REQ-143` 里用 `code` 刚建过的同一个 target dir 跑 `cargo build`，cargo 打印
`Fresh ontok v0.1.0 (…\feature-REQ-143)` **却没有重编**，产出的二进制 sha 与基线**完全相同**、
`--help` 里没有 `--allow-doc-only`。**那是一个"假特性二进制"**。
⇒ 每个 worktree（含变异 worktree）各用一个 target dir；本报告所有数字都是改正后重测的。

---

## 1 · 结论

> ## **FAIL** —— **1 条阻断（B-1）**；功能面 **8 条 AC 全部实证通过**，不退化成立。

| # | 项 | 判 |
| --- | --- | --- |
| 1 | 不开开关 ⇒ 零字节变化（R5 / AC-001） | ✅ **6/6 sha 相同** |
| 2 | 开开关 ⇒ 纯文档通路成立（AC-002 / AC-006） | ✅ c1 `2 实体 + 1 关系`；c3 `281 + 48` |
| 3 | key 确定性（AC-003） | ✅ 两次同 sha；换行序 key 集不变；变异红 |
| 4 | band ≤ medium / 未签（AC-004） | ✅ 329/329 `band=low`、`reviewed_by=null`；变异红 |
| 5 | 不造端点（AC-005） | ✅ 三种构造 + 真实样本 14 条 |
| 6 | 冲突 / 排除规则不变（AC-007） | ✅ |
| 7 | 端到端 + R-1 可溯源性 | ✅ `301 = 20 + 281`、`58 = 10 + 48`；281/281 可溯 |
| 8 | 不退化 / 零依赖 | ✅ 基线与特性各 733 / 742 条 lib 测试 0 失败 |
| — | **R-2 的差异写进 `--help` / 模块头** | ❌ **B-1 阻断** |
| — | R-1 / R-2 / R-3 裁定本身 | ✅ 按 BA 裁定执行，未回打 |

**B-1（阻断）**：R-2 裁定「接受名字优先于别名」的**追加要求**是「必须写进 `--help` 或模块头（⛔ 不能让它在暗处）」。
实测：`--help` 与模块头（`doc_merge.rs` 的 `//!` 块，1–112 行）**都没有**这条差异；模块头第 44–46 行反而写着
「其余规则一字不改」「概念命中实体（名字/别名/`maps_to`）｜与不开开关时**相同**」—— 与实现相反（见 §10）。
改动量 = 一处文档（模块头表格 + 一条 help 行），**不动代码、不动行为**，改完只需复跑 §10 的两条 grep。

---

## 2 · 零字节变化（R5 · 最硬的一条）

**做法**：基线二进制与特性二进制**都不传** `--allow-doc-only`，同输入、同参数，各写自己的
`merged.json` / `stats.json`，比 sha256。

### 2.1 sha 清单（6 类输出 = 3 组输入 × 2 个产物，逐字节相同）

| 输入 | 产物 | 基线 `f04b2ae` | 特性 `73e02d3` | 判 |
| --- | --- | --- | --- | --- |
| c1 3 断言 + 空提案 | `merged.json` | `f4115e94cfc0b8e0e7014e7eff44a31c26af03e929993311c2ffa61d32a4497f` | 同左 | ✅ |
| c1 | `stats.json` | `fa003e4d81c41732ea2b954d6d90675599739852513a70506efdea08e93f736b` | 同左 | ✅ |
| c2 56 行 + demo 提案 | `merged.json` | `7f8713a95f9ac637b5412551076fed5a6107365a99dd2a98f7b978f1cecc2573` | 同左 | ✅ |
| c2 | `stats.json` | `bdaca4cb1270f8ae28013883a161ebc880c8ae8b13fa06ed2685cf7eee7ec076` | 同左 | ✅ |
| c3 728 断言 + 30 条提案 | `merged.json` | `8aab94acc399f02835ca10e04a37e7f89ca6b4e232a78ebaa634a77f0183b407` | 同左 | ✅ |
| c3 | `stats.json` | `fdf7ee5cf1e6625855319e088ef7c0e0779a82b1f8c62100e95ef02bcf38a6b1` | 同左 | ✅ |

### 2.2 计数（AC-001 的三项）

| 输入 | `merged` | `merged_doc_only` | `doc_unmapped` | `items_added` | `assertions_read` |
| --- | --- | --- | --- | --- | --- |
| c1 | **0** | **0** | **3**（`no_maps_to` 1 · `no_maps_to_and_no_name_match` 2） | 0 | 3 |
| c2 | 14 | 0 | 14 | 0 | 56（排除 28） |
| c3 | 18 | 0 | 706 | 0 | 728（排除 4） |

c1 与 demand 里「并入 0 · 未映射 3 · doc-only 条目 0」逐字一致。

### 2.3 不开开关时 `stats` 里没有 `doc_only*` 字段（`skip_serializing_if`）

对三组输出的 `stats.json` + `merged.json` 搜
`"doc_only"` / `"doc_only_entities"` / `"doc_only_links"` / `"allow_doc_only"` / `"doc_only_band"`：

```
c1-three-empty : base_hits=0 feat_hits=0
c2-req116-56   : base_hits=0 feat_hits=0
c3-ops360-728  : base_hits=0 feat_hits=0
```

（`merged_doc_only` 是 REQ-129 的**老字段**，不是 REQ-143 的；上表用精确引号匹配排除了它的误命中。）

### 2.4 R5 的风险面：唯一两处"没加开关"的改动，我核过是不可观测的

| 改动 | 为什么不可观测 |
| --- | --- |
| `Index::build` 不再把**空列**的 `add_link_type` 放进 `links`（原实现会放） | `links` 全仓只有一处读：`resolve_relation` 之后的 `index.links.get(&(table, column, parent_table))`，而那个 `column` 来自 `maps_to.child.column`，**空值早已被 `bad_maps_to` 拒掉** ⇒ 空列条目永远查不到。c3 的 30 条老提案里**正好有 10 条空列关系**，输出仍逐字节相同，实测佐证。 |
| `by_name` 拆成 `by_name` + `by_alias`（别名单独建表） | 老路径统一走 `name_hits()`，它把两张表**并起来**（与旧实现等价）；`by_alias` 的"名字优先"只在 `--allow-doc-only` 分支里读。 |

---

## 3 · 纯文档通路真成立（AC-002 / AC-006）

### 3.1 c1（3 断言 + 空提案 + 开关）—— demand 点名的那组

```
merged=3 · merged_both=0 · merged_doc_only=3 · doc_unmapped=0 · items_added=3
doc_only=true · doc_only_entities=2 · doc_only_links=1
```

stderr：

```
  并入        : 3
  未映射      : 0
  合并为 both : 0 · doc-only 条目 3（新增 3 条）
  纯文档通路  : 开（新建实体 2 · 新建关系 1（无 join 列）· 档位 low）
```

新建条目原文（`merged.json`）：

```json
{ "kind": "add_entity_type", "target": "订单", "origin": "doc", "band": "low",
  "reviewed_by": null, "applicable": true,
  "payload": { "key": "doc_3d5436ea23d2", "display_name": "订单", "properties": [],
               "doc_evidence": [ { "kind": "concept", "name": "订单", "quote": "原文",
                                   "file": "01.md", "chunk_id": "abc:0-10", "span": [0,6] } ] } }

{ "kind": "add_link_type", "target": "订单→客户", "origin": "doc", "band": "low",
  "reviewed_by": null, "applicable": true,
  "payload": { "key": "rel_b06acde31d88",
               "from_type": "doc_3d5436ea23d2", "to_type": "doc_ad4af312dbc2",
               "cardinality": "N:1", "semantic": "下单",
               "evidence": [ { "table": "", "column": "", "claim": "文档断言（REQ-143 doc-only）：…" } ] } }
```

`stats.close()` 的闭合式（并入 + 未映射 + 冲突 + 排除 == 读入）在三组运行里都成立（否则命令会 bail）。

### 3.2 c3（728 真实断言 + 30 条老提案 + 开关）—— 按 **R-1 加总式**核

```
merged=572 · merged_both=7 · merged_doc_only=329 · conflicts=0 · excluded_not_alignable=4
doc_unmapped=152（endpoint_not_resolved 150 · doc_cardinality_conflict 2）
items_in=30 → items_out=359
doc_only_entities=281 · doc_only_links=48
```

**加总式核算（R-1 要求的三个数各归各，不拿"类数 = 概念数"去核）**

| 环节 | 数 | 说明 |
| --- | --- | --- |
| 概念断言 | **506** | 片段级 |
| 其中不可对齐 | **4** | 3 `future` + 1 `example`（`ops360-backup-coverage` / `in_development` / `ops360-report-generator` / `ops360-security-group-risk`） |
| 可对齐概念断言 | 502 | |
| **去重后概念名** | **288** | 与 R-1 的 288 一致（我一开始数出 289，是因为把 4 条不可对齐的也算进去了） |
| 其中命中老提案（走 `both`） | **7** | 备份策略 / 快照 / 关系型数据库实例 / 巡检报告 / 费用概览 / 云凭据 / 地域 |
| **新建 doc 实体** | **288 − 7 = 281** | 实测 `doc_only_entities=281` ✅ |
| 老提案实体 | **20** | 30 条提案里 20 条实体 |
| **TTL 类数** | **20 + 281 = 301** | 实测 `ontology export` 报 **301 类**；从 TTL 里独立数 `owl:Class` = **301** ✅ |
| 关系断言 | **222** | |
| 其中两端解析不到 | **150** | → `endpoint_not_resolved` |
| 其中 doc-vs-doc 基数打架 | **2** | → `doc_cardinality_conflict`（见 N-1） |
| 实际并入的关系断言 | 70 | 222 − 150 − 2 |
| 按 (from, predicate, to) 去重后 | **48** | 70 − 22 条重复主张（并入已有边） |
| 老提案关系 | **10** | |
| **TTL 关系数** | **10 + 48 = 58** | 实测 `ontology export` 报 **58 关系**；TTL 内 `owl:ObjectProperty` 块 116 个 = 58 × 2（正向 + `owl:inverseOf` 反向，REQ-119 行为）✅ |

⇒ **AC-006 的判据（R-1 口径）逐项吻合，且 288 / 281 / 301 / 48 / 58 五个数互相自洽。**

---

## 4 · key 确定性（AC-003）

### 4.1 同一输入两次 ⇒ 逐字节相同

| 输入 | `merged.json` | `stats.json` |
| --- | --- | --- |
| c1 | `68d649c6fc1977afe01d3451d825cf12d5679397a1f81444dbfeb22594c34e44`（两次同） | `8a1a7a967bb591d88a5ed786d3b8474402f00737a52ecc22ed07f2fe52c29dba`（两次同） |
| c3 | `d7686cc69e49089a3e7d96a148d56da87c8ba4364b56b0a2d0e489dd0911b119`（两次同） | `57dab9d1ef535c460a87dfe260d42b65bb1c6bd079d3708c31c35677a7732203`（两次同） |

### 4.2 打乱断言行序 ⇒ key 集合不变

用固定种子（`System.Random 143`）打乱 c3 的 728 行后重跑：

```
orig keys=359  shuffled keys=359
key-set identical: YES          （Compare-Object 无差异）
entities: 281 vs 281 · links: 48 vs 48
```

### 4.3 key 派生规则（只由名字/内容决定）

```rust
doc_entity_key(name) = "doc_" + ascii_slug(name) + "_" + sha256(name)[..12]   // 纯中文 ⇒ slug 空 ⇒ "doc_<hash>"
doc_link_key(from,pred,to) = "rel_" + sha256("{from}\x1f{predicate}\x1f{to}")[..12]
```

无序号、无时间、无随机、无输入顺序；哈希前缀撞车时按**名字字典序**加长到 24/64 位（`BTreeMap` 迭代 ⇒ 与行序无关）。
金值：`doc_entity_key("订单")="doc_3d5436ea23d2"`、`("客户")="doc_ad4af312dbc2"`、`("CLI")="doc_cli_e759793341ff"`。

### 4.4 变异 G②（key 掺 pid）⇒ **必须红** —— ✅ 红

在独立 worktree（`.local/wt-req143-mut`，分支 `qa/REQ-143-mut` @ `73e02d3`）上把
`derive_doc_key` 的 digest 改成 `sha256("{name}#{pid}")`：

```
test result: FAILED. 0 passed; 1 failed; ...
test extract::doc_merge::tests::doc_only_keys_depend_only_on_the_names ... FAILED
  panicked at src\extract\doc_merge.rs:3400: assertion `left == right` failed
```

还原后 `git status --porcelain` 为空（无残留）。

---

## 5 · band 与未签（AC-004）

### 5.1 逐条断言（c3 全量 329 条 `origin: doc`，不是抽样）

| 判据 | 实测 |
| --- | --- |
| `origin: doc` 条目数 | **329**（`add_entity_type` 281 · `add_link_type` 48） |
| `band` 取值分布 | **`low` = 329**（无 `medium`，无 `strong`） |
| `band ∉ {low, medium}` | **0** |
| `reviewed_by` 缺失或非 `null` | **0**（全部显式 `null`） |
| 关系 `payload.evidence[0].column` 非空串 | **0**（48/48 是 `""`） |
| 关系 `payload.evidence[0].table` 非空串 | **0**（48/48 是 `""`；⛔ 没拿谓词冒充列） |
| 新建实体 `properties` 非空 | **0**（281/281 是 `[]`，⛔ 不编字段） |
| `applicable:false` 混进 doc 条目 | **0** |

`merged.json` 的 `doc_merge` 摘要块也自证档位上限：`"doc_only_band": "low"`、`"band_cap": "medium"`。

### 5.2 变异 G③（doc 条目设 `strong`）⇒ **必须红** —— ✅ 红

`DOC_ONLY_BAND: "low" → "strong"`：

```
test result: FAILED. 0 passed; 1 failed; ...
test extract::doc_merge::tests::doc_only_items_never_carry_a_band_stronger_than_medium ... FAILED
  panicked at src\extract\doc_merge.rs:3492: assertion `left == right` failed
```

### 5.3 附加变异 G①（开关默认开）⇒ 必须红 —— ✅ 红

把 CLI 的 `allow_doc_only` 写死 `true`（模拟"默认开"）后重跑 c1 **不传开关**：

| | 变异版 | 基线默认关闭 | 判 |
| --- | --- | --- | --- |
| `merged.json` | `68d649c6fc19…4e44` | `f4115e94cfc0…497f` | **DIFF** ✅ |
| `stats.json` | `8a1a7a967bb5…9dba` | `fa003e4d81c4…736b` | **DIFF** ✅ |

变异版 sha 与"合法开开关"那次运行**完全相同**（`68d649c6…`），说明差异确实来自那条通路。
⇒ 我 §2 的逐字节判据**不是空转**。

---

## 6 · 不造端点（AC-005）

| 构造 | 结果 |
| --- | --- |
| 只有「客户」概念 + 关系 `订单 -[下单]-> 客户`（**subject 不存在**） | 新建实体 1 · **新建关系 0** · `doc_unmapped 1`（`endpoint_not_resolved` 1）· `@graph/items` 里**一条边都没有**（`items_out=1`，kind 只有 `add_entity_type`） |
| 只有「订单」概念 + 同一条关系（**object 不存在**） | 同上：实体 1 · 关系 0 · `endpoint_not_resolved 1` · `items_out=1` |
| **真实样本**：c2（56 行 req116 fixture + demo 提案）。14 条关系都是 `客户 -[下单]-> 订单`，「客户」命中老实体 `customer`，但**「订单」在断言里从未作为概念出现** | 14 条关系**全部**进 `doc_unmapped`（`endpoint_not_resolved 14`），`doc_only_links=0`；**没有凭空造出「订单」这个类**，也没有边 |

stderr 现象也自证可读性：

```
  ⚠ 端点未解析: 14 条关系只解析到一端 ⇒ 仍进 doc_unmapped（⛔ 不凭空造端点）
```

---

## 7 · 冲突 / 排除规则不变（AC-007）

### 7.1 冲突：双保留 + `applicable:false`

夹具：`mini-proposal.json`（`order` N:1 `customers`）+ `conflict.jsonl`（doc 说 `1:1`，带 `maps_to`）。开开关跑：

```
merged=0 · conflicts=1 · doc_unmapped=0 · doc_only_entities=0
kind=add_link_type origin=both band=medium applicable=False key=order_customer_id__customers card=N:1
conflicts[] = {"subject":"订单","predicate":"下单","object":"客户","doc_says":"1:1","db_says":"N:1",
               "quote":"客户下单","file":"01.md","span":[20,24],...}
```

两侧说法都在（`doc_says` / `db_says`），DB 的 `cardinality` 没被 doc 覆盖，条目 `applicable:false`。
**apply 侧拒绝并点名**由 `test_req129_doc_merge::ac006_applier_refuses_an_inapplicable_item_and_names_it` 覆盖，本轮 **21/21 全绿**（见 §9）。

### 7.2 不可对齐仍排除并计数

夹具：`alignable:false` + `future` + 一条正常概念，开开关：

```
排除不可对齐: 2（future 1 · example 0 · 模态与标记矛盾 2）
doc_only_entities=1 · items_out=1 · doc_unmapped=0
```

⇒ `alignable:false` **没有**因为开关被造成类；只有那条正常概念建了实体。

---

## 8 · 端到端 + R-1 可溯源性

### 8.1 全链路（8 步全 `exit=0`）

用 c3 的 `merged.json`（`project` 改名为 `req143_qa` 后**重新跑 merge**，产物是产品自己写的，**没有手改 JSON**）：

```
ontok doc merge --allow-doc-only                      # 281 实体 + 48 关系
ontok doc sign  × 329（逐条，--by qa-req143 --at 2026-10-25T05:00:00Z）   # ⛔ 没手工改 reviewed_by
ontok project new --name req143_qa
ontok version new --project req143_qa
ontok propose import --file signed.json               # 提案 p1 已导入，共 359 个条目
ontok review decide --proposal p1 --accept-all --project req143_qa   # 359 × 2 轨 → accepted
ontok proposal apply --proposal p1 --project req143_qa               # → v2
ontok version freeze --project req143_qa --version 2                 # 已冻结 v2
ontok ontology export --project req143_qa --version 2 [--model-view] --out …ttl
```

签字结果：`signed doc items: 329 / 329`，`signed_at` 唯一（`--at` 可复现），`signed_at_source=arg`；
30 条 DB 轨条目**照旧未签**（两条轨没有混）。

### 8.2 TTL 头部计数 + `catalog` 口径

```
· 已写出建模视图 TTL：req143_qa-v2-model.ttl（301 类 / 58 关系 · 有损项见文件头）
· 已写出客户视图 TTL：req143_qa-v2-customer.ttl（301 个对象 / 58 条关系 · 有损项见文件头）
```

我从 TTL 里**独立**数（不信 stderr）：`a owl:Class` = **301**；`owl:ObjectProperty` 块 = **116** = 58 × 2
（正向 + 反向 `owl:inverseOf`）⇒ 关系 **58**。两个视图的类/关系/属性计数一致（`DatatypeProperty` 各 47）。

### 8.3 R-1 的**可溯源性**：每个 `origin: doc` 实体都能找到同名概念断言

```
doc entities=281
断言里去重后的可对齐概念名=288
doc entities WITHOUT a same-named concept assertion: 0
```

**抽样 5 个**（固定种子随机抽，非挑选）：

| # | key | `display_name` | `band` | `reviewed_by` | 同名概念断言条数 |
| --- | --- | --- | --- | --- | --- |
| 1 | `doc_e32b9aa6d8f7` | 会话凭据 | low | null | 5 |
| 2 | `doc_inventory_counts_by_type_ssl_56aaca050b9e` | `inventory.counts_by_type.ssl` | low | null | 1 |
| 3 | `doc_a3be1a3fcfab` | 风险清单 | low | null | 1 |
| 4 | `doc_19ce33f0059b` | 账单窗口 | low | null | 1 |
| 5 | `doc_e9bf2c2feae4` | 周期 | low | null | 2 |

关系抽样 5 条（端点都用 key 反查到实体）：

```
rel_8d5494a47d11  timestamp_convert.py -[写进]-> --output-dir      column='' table='' card=N:1
rel_681e4884a3e1  facts -[输出]-> abnormal_status_count            column='' table='' card=N:1
rel_ceaafcd8067e  errors -[记录]-> instance_count_exceeded          column='' table='' card=N:1
rel_88b59b9f2944  timestamp_convert.py -[写入]-> analysis            column='' table='' card=1:1
rel_078d4e99bc6b  只读约束 -[允许执行]-> Describe、List、Query、Get 类命令  column='' table='' card=1:N
```

### 8.4 关于 AC-006 原文的「与脚本老路径 TTL 等价」

按 R-1 已改成加总式，这一条**不按"总类数相等"核**。实测老脚本产物
`.local/ops360/ops360.ttl` 是 **20 类 / 4 个 ObjectProperty 块（=2 关系）/ 47 属性**
—— 因为那个 Python 脚本只挑了 20 个概念，本来就不是全量。可核的是**加总式**（§3.2，全部吻合）。

### 8.5 R-3 的旁证（只记录，不回打裁定）

客户视图导出时发布闸**确实命中**了：`⚠ 发布闸前处理：共剔除 10 项（… 只隐去说明 10 …）`，
10 条全部来自**老提案**条目（它们带 `description`）。REQ-143 新建的 doc 实体**没有** `description`
（描述只留在 `doc_evidence[]`），因此**一条都没被闸拦**。⇒ 不做 `rdfs:comment` 搬移这个选择是**安全侧**的。

---

## 9 · 不退化 / 零依赖

**基线数字全部自己实测**（同一台机、同一工具链；缺 `ONTOK_TEST_DATABASE_URL` 时两条 DB 用例会 panic，故两边都带上变量跑）：

| 项 | 基线 `f04b2ae` | 特性 `73e02d3` | 判 |
| --- | --- | --- | --- |
| `cargo test --lib`（全量） | **733 passed / 0 failed** | **742 passed / 0 failed**（+9） | ✅ |
| 其中 `doc_merge::tests` | **33** | **42**（+9） | ✅ 只增不减 |
| `cargo test --test test_req129_doc_merge` | 21（该测试文件 **零 diff**，故条数相同） | **21 passed / 0 failed** | ✅ |
| `cargo test --test test_property_filter` | — | **10 passed / 0 failed** | ✅ G1–G4 未动 |
| `cargo test --test test_req107_red_lines` | — | **2 passed / 0 failed** | ✅ 它自己就在核红线 |
| 新增 `#[ignore]` | — | **0** | ✅ |
| 删除 `#[test]` | — | **0** | ✅ |
| 新依赖 | — | `Cargo.toml` / `Cargo.lock` **zero diff** | ✅ |

`git diff --numstat f04b2ae 73e02d3` 全量只有两行（`57 1 src/cli.rs`、`988 37 src/extract/doc_merge.rs`），
逐路径核过**全部 zero diff**：

| 禁改项 | 结果 |
| --- | --- |
| 四红线：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs`、`src/domain/review_view.rs` | ZERO DIFF |
| `src/mcp/tools.rs`（`pub fn guard(value:&Value)` 在 693 行） | ZERO DIFF |
| `src/domain/property_filter.rs` | ZERO DIFF |
| `src/extract/doc_ingest.rs`（⛔ 不改 `doc ingest`） | ZERO DIFF |
| `Cargo.toml` / `Cargo.lock` | ZERO DIFF |

---

## 10 · R-2 的差异是否写进帮助 —— ❌ **没有（B-1）**

**R-2 的要求**：「打开开关时解析优先级从『名字 ∪ 别名』变成『名字优先于别名』，**这个差异必须写进 `--help` 或模块头**」。

**实测（三条 grep，全部为空/相悖）**：

| 位置 | 结果 |
| --- | --- |
| `ontok doc merge --help` | 搜 `优先` / `名字优先` ⇒ **0 命中**。help 里只有老文案「概念只按「精确名字/别名命中」对齐」，读起来是"名字和别名平权"。 |
| 模块头 `doc_merge.rs` 的 `//!` 块（1–112 行） | 搜 `优先` ⇒ **0 命中**。 |
| **模块头第 44–46 行（相反）** | `//! 打开 --allow-doc-only 之后**只多一条出口**，其余规则一字不改：` / `//! \| 概念命中实体（名字/别名/maps_to） \| 与不开开关时**相同**…` —— **与实现相反**。 |

**差异实际只写在**：`Index::doc_keys` 字段注释（596 行）、`resolve_mention()` 的函数文档（770 行）、
`resolve_concept` 内的行内注释（887 行）、以及测试的文档注释（3526 行）。
这些都不是 BA 说的「`--help` 或模块头」，FDE 读 `--help` 时看不到。

**为什么这一条不能放过**：真实样本里正是靠它避免 13 条概念被挤成 `ambiguous_name`
（`CLI` 与 `CLI 直采` 的别名冲突，已有专测 `doc_only_resolves_each_concept_by_its_own_name_even_when_an_alias_collides`）。
它是**被接受的偏离**，越是偏离越不能只在代码注释里；help 里现在那句「其余规则一字不改 / 与不开开关时相同」会直接误导排查。

**建议改法（不动代码）**：模块头表格补一行

```
| **名字/别名优先级** | 开关下：先按**名字**解析（唯一命中即采用），名字不中才看**别名**；歧义仍 `ambiguous_name`。
  不开开关时仍是「名字 ∪ 别名」并起来看（REQ-129 行为，R5） |
```

`--help` 的 REQ-143 段补一行同义说明；顺手把第 46 行的「与不开开关时**相同**」限定为「除下面的优先级差异外」。

---

## 11 · 我的临时目录 & 是否清理

全部在 `.local/`（根 `.gitignore` 已忽略，不进任何分支）：

| 路径 | 内容 | 处置 |
| --- | --- | --- |
| `.local\cargo-qa143-1`（2.93 GB） | 基线二进制构建（`code` @ `f04b2ae`） | ✅ **已删除** |
| `.local\cargo-qa143-2`（4.30 GB） | 特性二进制 + lib/集成测试构建 | ✅ **已删除** |
| `.local\cargo-qa143-mut`（3.50 GB） | 变异 worktree 构建 | ✅ **已删除** |
| `.local\wt-req143-mut` | **变异 worktree**（分支 `qa/REQ-143-mut` @ `73e02d3`） | ✅ **已 `git worktree remove --force` + `git branch -D qa/REQ-143-mut`**；`git worktree list` 只剩 workspace / BA / code / Deploy / feature-REQ-143 |
| `.local\qa143\in\` | 三组输入 + AC-005/AC-007 构造夹具（含 `ops360-shuffled.jsonl`） | **保留**（很小，可复验） |
| `.local\qa143\base\ontok.exe` / `.local\qa143\feat\ontok.exe` | 两个自建二进制（24 MB × 2） | **保留**（复验直接对拍，不用再编） |
| `.local\qa143\out\`（≈1.4 MB） | 全部证据：6 组 R5 输出、`on-c1/on-c2/on-c3`、`ac005a/ac005b`、`ac007-*`、`det-*`、`shuf-c3`、`mut-c1`、`e2e\`（merged/signed/stats/两版 TTL/pipeline.log）、`help-merge.txt` | **保留**（报告每个数字都能回查） |
| `.local\qa143\e2e.ps1` `.local\qa143\mutate.ps1` | 端到端与变异脚本 | 保留 |

清理后 `.local\qa143\` 共 **50.7 MB**（保留物：两个二进制 + 全部证据），三个 cargo target dir 已删、盘上回收约 **10.7 GB**。
feature 分支工作区 `git status --porcelain` 为空、HEAD 仍 `73e02d3`（⛔ 未改、未 push）。

**数据库**：新建了项目 `req143_qa`（id `1a3123fb-88ff-4ab0-aab0-1fe1410e431a`，v1 + v2 已冻结）。
`ontok project` 只有 `new` / `list`，**没有删除命令** ⇒ 这个 QA 项目会留在 `anspire_real` 里（名字带 `qa`、不影响 `ops360` / `anspire_*`）。
另外：`project list` 里另有 Dev 自己建的 `req143_doconly` / `req143_doconly_final`，我**没有动**它们。

⛔ 全程：**没有**改 feature 分支、**没有** push、**没有**在 `D:\` 根建目录、**没有**碰 `ops360` / `anspire_r2rml_excl`。

---

## 12 · 非阻断发现（建议改，但不拦 merge）

### N-1 · doc-vs-doc 基数打架：**第一个看到的说法赢**，且不留 `conflicts[]`

真实样本里 222 条可对齐关系中有 **12 组**「同一 (subject, predicate, object) 但基数不一致」，
其中 **2 组**真的触发了这个判定（`stats` 里 `doc_cardinality_conflict=2`；其余组的端点先就没解析到，
落到 `endpoint_not_resolved`）。触发的那条分支在 `merge_doc_only_relation` 里
（`prior.and_then(origin_of_value) == Some(ORIGIN_DOC)`）：
**后到的说法直接丢弃、只记一个计数**，不进 `conflicts[]`、不设 `applicable:false`。

两个小问题：

1. **与模块自己的原则不一致**：模块头对冲突说「挑一边是**人的决定**，不是合并器的」；这里合并器替人挑了（先到者），
   而且真实样本里"先到者"可能正是缺 `cardinality`、被默认成 `N:1` 的那条。
2. **轻微顺序敏感**：同一份 728 行，原序 `doc_cardinality_conflict=2`、打乱后 `=3`（`merged` 572 vs 571），
   **key 集合不变**（AC-003 只要求 key 集，故不算违规）。若 `doc ingest` 换了行序，`stats` 会飘 1。

建议：把 doc-vs-doc 的分歧也写进 `conflicts[]`（至少保留两侧说法），或按确定性规则（如取两侧里更具体的那个）而不是"先到者"；
新码 `doc_cardinality_conflict` 补进模块头的原因码清单（现在只有 `endpoint_not_resolved` 被写进 head/help）。

### N-2 · doc-only 不禁自环（老路径有 `self_link` 规则）

老路径的 `maps_to` 校验里有 `self_link`（child 表 == parent 表 ⇒ 未映射）。doc-only 通路没有这条：
真实样本里建出了 **1 条自环边** `doc_bucket_faf330ab41fa -[不可写入]-> 同一实体`（`rel_abc5e5f0979f`）。
语义上怪，但不违反任何 AC（OWL 允许），导出也正常。建议要么在 doc-only 里同样排除并计数（`self_link`），
要么在模块头写明"自环允许"。

### N-3 · `--allow-doc-only` 与 `--schema` 同用的口径已在 help 写清（不改）

help 明确「`--schema` 的严格面只作用于既有实体的 `maps_to`」，与实现一致（新建实体本来不在任何库里）。✅ 不是问题，只是记录我核过。

---

## 附录 · 复验命令（可直接照抄）

```powershell
# 0) 两个 worktree 各用一个 target dir（⛔ 不要共用，见 §0 的坑）
$env:CARGO_TARGET_DIR='D:\MyCodes\AI\an_onto_demo\.local\cargo-qa143-1'
cd D:\MyCodes\AI\an_onto_demo\code;               cargo build --bin ontok   # 基线 f04b2ae
$env:CARGO_TARGET_DIR='D:\MyCodes\AI\an_onto_demo\.local\cargo-qa143-2'
cd D:\MyCodes\AI\an_onto_demo\feature-REQ-143;    cargo build --bin ontok   # 特性 73e02d3

# 1) R5：两组二进制都不传开关，比 merged.json / stats.json 的 sha256
#    输入：.local\qa143\in\{three.jsonl,empty-proposal.json} / {req116.jsonl,proposal-demo.json}
#          / {ops360.jsonl,proposal-ops360.json}

# 2) 纯文档通路
ontok doc merge --assertions …\three.jsonl --proposal …\empty-proposal.json --out … --stats … --allow-doc-only

# 3) 端到端（库）：project new → version new → propose import → review decide --accept-all
#                → proposal apply → version freeze → ontology export [--model-view]
```

**证据目录**：`.local\qa143\out\`（sha 可直接 `Get-FileHash` 复算）；脚本 `.local\qa143\e2e.ps1`、`.local\qa143\mutate.ps1`。
