# QA 验收报告 · REQ-055 建模轨投影

- 验收对象：`verify/REQ-055` @ `d3ebea8`（基线 `7343bb0`）
- worktree：`/home/myg133/Projects/Anchnet/Ontology/verify-REQ-055`
- `target/` 起始**不存在**、非 symlink ⇒ 本地从零编译（1m12s）
- 环境变量：`ONTOK_TEST_DATABASE_URL` 取仓库根 `.env` 并把 `/notol` 换成 `/ontol`（实测串长 59、以 `/ontol` 结尾、不含 `notol` 子串；口令未回显）
- 纪律偏离登记：本环境**只提供 read/bash/grep/glob**，无 `edit`/`write` 工具 ⇒ 源码变异用 `sed`、报告用带引号 heredoc 落盘（与「用 edit/write 写 Markdown」一条的兜底条款一致）

## 结论

**PASS 带非阻断项。** AC-001 / AC-002 / AC-003 全部通过；Dev 自报的 P1 属实且**不属于本需求范围**，建议另开（详见第 4 节）。

---

## 1 · AC-001 判别力成立吗？—— 成立

### 1.1 控制实验是否真的控制住了

变量只有「用哪个投影」这一条，其余（同一 `Snapshot` 实例、同一个 `scan_modeling_terms`、同一 `serde_json::to_string`）确实相同：

- `tests/test_model_view.rs:201` 取一次 `let snap = fixture_snapshot();`
- `:203` 建模轨 `project_model_track(&snap)`；`:204` 客户视图 `project(&to_customer_input(&snap))`
- `:206`/`:207` 两边调**同一个** `scan_modeling_terms`

⚠️ **但有一处未被控制住的变量，必须记录**：客户视图喂进去的不是 `&Snapshot` 而是 `SnapshotView`，中间隔着测试内自带的转换函数 `to_customer_input()`（`tests/test_model_view.rs:103-141`）。该函数是 `src/cli.rs::snapshot_to_view` 的**同形副本**。所以严格讲存在「转换函数本身有损」这一条理论通道。

**独立核实：这条通道对本 fixture 不产生差异。** `to_customer_input` 逐字段搬运 `key/display_name/description/aliases/parent/disjoint_with/properties/evidence` 与 `from_type/to_type/cardinality/semantic`，无丢弃、无改写（`:105-140` 逐条对照可见）。且客户视图那侧断言 `leaked.is_empty()` **通过了**（`test_customer_view_output_carries_none_of_them ... ok`）—— 若转换有损到把建模术语抹掉，那正是「客户视图干净」这条要证明的事，反过来无风险。

**fixture 真含建模术语**（`tests/test_model_view.rs:36-96`）：`key=customer`/`subscription`、列名 `customer_pk`/`customer_id`、`cardinality="1:N"`、`semantic="has"`、`disjoint_with=["supplier"]`、`evidence` 非空、`extracted_by{agent,version,round,failed_groups}` 非空。**含建模术语这一点成立**。

### 1.2 三条变异（独立复现，均先编译通过再执行）

| 变异 | 改法 | 编译 | 二进制 | 运行条数 | 红了几条 |
|---|---|---|---|---|---|
| **M-A** 抽掉 `column` | `src/domain/model_view.rs:145` `column: p.column.clone()` → `String::new()` | ✅ Finished 13.35s | `test_model_view` | 10 | **3** |
| **M-B** 基数翻人话 | `:161` `cardinality: l.cardinality.clone()` → `"一对多".into()` | ✅ Finished 17.51s | `test_model_view` | 10 | **2** |
| **M-C** 照抄客户视图取舍 | 同时抽 `key`/`disjoint_with`/`evidence`(实体+关系)/`column` + 基数翻人话 | ✅ Finished 18.45s | `test_model_view` | 10 | **4** |

杀死它们的判据（全部在 `test_model_view` 这一个二进制内）：

- **M-A ×3** → `test_model_track_invents_no_primary_key_field`、`test_model_track_keeps_what_customer_view_drops`、`test_model_view_output_carries_modeling_terms`
- **M-B ×2** → `test_model_track_keeps_what_customer_view_drops`、`test_model_view_same_snapshot_terms_land_in_model_track_only`
- **M-C ×4** → 上述两条 + `test_model_view_output_carries_modeling_terms` + `test_model_view_same_snapshot_terms_land_in_model_track_only`

**存活变异论证（唯一一处）：** M-A 下 `test_model_view_same_snapshot_terms_land_in_model_track_only` **存活**。原因是该测试只要求「命中非空」时，建模轨结构里 `key`/`evidence`/`extracted_by`/`cardinality`/`disjoint_with` 这些**字段名本身**就足以把守门器点亮，抽掉 `column` 的数据不影响它。**这与 Dev 的自述一致**，且加固判据正是为补这条而加。

### 1.3 与 Dev 自述的一处出入（已核实，属报告口径过时）

Dev 报 **M-B ×1**；我在**加固后的最终代码**上实测 **×2**。差异原因是加固前判别力测试在 M-B 下存活（故只有字段级对照那一条会红），加固后 `1:N` 数据被翻掉 ⇒ 判别力测试**一并转红**。

⇒ 这不是缺陷，反而**正面证实加固确实生效**：加固把 M-B 的杀死能力从 1 条提到 2 条，且新增的正是「必须有一条命中来自数据」这条。**建议 BA 在采纳时以 ×2 为准**。

### 1.4 加固判据够不够？—— 够，但非「不可伪造」

加固判据（`tests/test_model_view.rs:225-229`）：命中里必须有一条 `h.contains("1:N")`。

独立构造的「只有数据触发、字段名不触发」反例（临时探针，已删除）：

| 探针 | 输入 | 实测命中 | 结论 |
|---|---|---|---|
| A1 | `{"how_many":"1:N"}` | `["未翻译的基数记号 1:N"]` | ✅ **形态守门确实只认数据**，JSON 里没有任何黑名单字段名 |
| A2 | `{"how_many":"一对多"}` | `[]` | 对照组干净 |
| A3 | `{"key":"","evidence":[],"cardinality":"","extracted_by":null}` | `["key"]` | ✅ **退化实现（只剩字段名）没有 `1:N`** ⇒ 加固判据**确能区分** |
| B | `{"1:N":"x"}` | `["未翻译的基数记号 1:N"]` | ⚠️ 见下 |

**A3 是关键**：它精确复现了 Dev 担心的那种退化实现，并证明加固判据会把它挡住。判据有效。

⚠️ **B 探针记录一个理论边界**：`cardinality_notation_hit`（`src/domain/view.rs:922-941`）扫的是整段文本、**不区分键与值**，所以字段名字面量叫 `1:N` 也会触发形态守门 ⇒ 加固判据**在绝对意义上不是不可伪造**。但这需要一个刻意取这种名字的字段，现实中不会发生；而且即便绕过，`test_model_track_keeps_what_customer_view_drops`（`:272/:276` 等）另行钉住了 `column=="customer_pk"`、`cardinality=="1:N"`、`extracted_by` 各子项等**真实数据**，单靠空壳过不去。**结论：够用**，此条按非阻断记录。

---

## 2 · 分离机制真的没放宽守门吗？—— 没放宽，且不只是「约定」

### 2.1 建模轨输出真的从未经过守门器

- 裸 `grep -n "scan_modeling_terms" src/domain/model_view.rs`：**只在第 21 行的文档注释里出现一次**，无任何代码调用。
- `src/domain/model_view.rs:42,44` 的 `use` 只有 `serde` 与 `crate::domain::snapshot` 若干类型。
- `grep -n "CustomerView\|view::" src/domain/model_view.rs`：命中的 6 处（11/19/20/21/115/120）**全部在文档注释里**，无一处代码依赖。

⇒ 「建模轨那份从不经过 `scan_modeling_terms`」**属实**。

### 2.2 客户视图的三条生产通道**全部**被守门覆盖（这才是分离的真正骨架）

`grep -rn "view::project" src/` 得 3 个生产调用点，逐一追到守门：

| 调用点 | 守门位置 |
|---|---|
| `src/cli.rs:712`（`ontology show` JSON） | `src/cli.rs:779` |
| `src/cli.rs:712`（`ontology show --html`） | `src/cli.rs:748`（先出页面再扫整段 HTML，fail-closed） |
| `src/cli.rs:801`（`ontology overview`） | `src/cli.rs:807` + `:809` |
| `src/mcp/serve.rs:89`（MCP） | 存入 ctx（`:115`）→ 每个返回值过 `tools::guard`（`src/mcp/serve.rs:273` → `src/mcp/tools.rs:611-628`） |

⇒ **没有一条客户视图通道是裸奔的。**

### 2.3 独立构造绕过（临时探针，已删除）—— 守门拦得住

试图让建模术语经客户视图**合法存在的字段**（含深层间接）流出去：

| 探针 | 输入 | 实测命中 |
|---|---|---|
| C1 | `{"objects":[{"display_name":"客户","description":"这条说明提到了主键"}]}` | `["主键"]` ✅ |
| C2 | `{"objects":[{"properties":[{"note":"外键约束"}]}]}` | `["外键"]` ✅ |
| C3 | `{"key":"customer","evidence":[...],"cardinality":"1:N"}` | `["key","未翻译的基数记号 1:N"]` ✅ |

守门器扫的是**序列化后的整段文本**、不依赖字段白名单，所以**换哪个字段藏都拦得住**，间接嵌套也拦得住。

### 2.4 「两个类型 + 两条通道」是结构性的吗？—— **结构性，但要说清是哪一层在承重**

- **类型层是真结构性的**：`ModelTrackView` 与 `CustomerView` 是互不相干的结构体，`project_model_track` 与 `project` 无互相调用，编译器保证建模轨那份**塞不进**客户视图的类型签名。
- **但「建模轨不过守门器」这一条本身是代码路径上的 `return`（`src/cli.rs:705`），不是类型强制。** 若有人日后把 `ModelTrackView` 的字段**加进** `CustomerView`，编译器不会拦。

**真正兜底的是运行时那层 fail-closed 守门**（2.2 + 2.3 已实测）。所以结论是：**分离有结构性承重 —— 但承重的是「客户视图通道上的运行时守门」，不是「类型不同」这一条。** Dev 的说法（「靠类型不同 + 通道不同」）成立，不过若哪天有人删掉 `cli.rs:779/748/807`，类型层**不会**替代它报警。这属于既有架构的既有属性，本需求未削弱，记为非阻断观察。

---

## 3 · domain 层纯度判据成立吗？—— 成立

- 裸 `grep -nE 'sqlx|PgStore|db::' src/domain/model_view.rs` ⇒ **exit 1（零命中）**。
- 全 `src/domain/` 共 4 处命中，**全部在文档注释里**：`apply.rs:6`、`snapshot.rs:3`、`snapshot.rs:16`、`mod.rs:3`。

**判据位置与其覆盖面**：`tests/test_extraction_agent.rs:1105-1136` 的 `test_domain_layer_stays_io_free` 用 `std::fs::read_dir("src/domain")` 遍历 + `extension == "rs"` 过滤 ⇒ **新增文件自动纳入**（目录扫描，非硬编码文件清单）。且 `:1121` 显式跳过 `//`/`/*`/`*` 开头的注释行，这正是上面 4 处注释不会误伤的原因。**判据覆盖新文件，已核实。**

---

## 4 · Dev 说的「顺带修正产品缺陷」属实吗？—— 属实，但有一处未声明的能力收缩

**属实**（`git diff 7343bb0 d3ebea8 -- src/cli.rs`）：

- 改前：`println!("{}", serde_json::to_string_pretty(&snap)?);` ← 存储层 dump
- 改后（`src/cli.rs:703-704`）：`project_model_track(&snap)` → `to_string_pretty(&mv)`

Dev 的描述（给 FDE 的是存储层 dump、不是可裁决投影）与代码一致。

**⚠️ 这个改动本身的风险（原需求与 Dev 报告均未提及）：**

对照 `Snapshot`（`src/domain/snapshot.rs:258-284`）与 `ModelTrackView`（`src/domain/model_view.rs:48-63`）的字段：

| `Snapshot` 有 | `ModelTrackView` 是否保留 |
|---|---|
| `version_id` | ❌ **丢** |
| `concept_groups` | ❌ **丢** |
| `seq`/`extracted_by`/`entity_types`/`link_types`/`action_types`/`functions` | ✅ 全留 |

（`EntityType`/`LinkType` 的**每一个**字段都被逐字段搬运，无丢失。）

而 `grep -n "concept_groups" src/cli.rs` 只有 `802` 一处（经 `ontology overview` 的业务口径），`grep` 也确认 **`ontology show --model-view` 是全 CLI 唯一的原始快照 dump**，改后该能力**整体消失**。

**影响面**：FDE 审建模质量用不到 `concept_groups`（那是业务确认层，属业务轨）与 `version_id`（`seq` + `--project/--version` 已能定位），故**方向正确、不阻断**；但这是**一次未声明的能力收缩**，操作者若曾把 `--model-view` 当调试 dump 用会察觉。**建议**：在 `--model-view` 的帮助文本或模块文档里写明「这是建模轨投影，不是原始快照；概念组请用 `ontology overview`」，避免后来者以为漏了。

---

## 5 · 全量、基线对照、红线、产物 sha

### 5.1 全量（2 轮）

口径：`cargo test --no-fail-fast`，逐行 `grep -E "^test result:"` 后用 **awk 逐行累加**（未用 `paste -sd+`）。

| 轮次 | passed | failed | ignored | 二进制数 | 备注 |
|---|---|---|---|---|---|
| 第 1 轮 | 583 | **1** | 1 | 42 | `error: 1 target failed: --test test_project` |
| 第 2 轮 | **584** | **0** | **1** | 42 | 全绿 |

**环境类失败（非代码失败）**：第 1 轮唯一失败是 `--test test_project`。第 2 轮同一二进制 `test_project` = `10 passed; 0 failed`（28.36s）。失败在两轮之间**不复现**，且与本次改动**无代码路径关联**（本需求只加了一个纯函数投影 + 一个 CLI 分支，未触碰 project 路径）⇒ 归类为**环境类**（Dev 报 `Connection reset by peer`，机制一致）。**不声称稳定复现率**——本会话只观察到 1/2 轮出现。

### 5.2 基线对照（574 → 584）

**静态证明**（比直接跑基线更严谨，因基线需另建 target）：

- `git diff --numstat 7343bb0 d3ebea8 -- tests/` ⇒ **`398  0  tests/test_model_view.rs`** —— 测试改动**纯增量、仅新增这一个文件、0 删除**。
- `git diff 7343bb0 d3ebea8 -- tests/ | grep -c "^-[^-]"` ⇒ **0**（无任何删除行）。
- `grep -c "^#\[test\]" tests/test_model_view.rs` ⇒ **10**。

⇒ 基线 = 584 − 10 = **574**，与 AC-003 所述基线 `574/0/1` **一致**。Dev 的「条数变了请同步文档」**属实且必要**。

### 5.3 隔离复跑（AC-002，命令 `cargo test --test <name>`）

| 二进制 | passed/failed/ignored | 与 Dev 自述 |
|---|---|---|
| `test_view` | 26/0/0 | ✅ 一致 |
| `test_mcp_server` | 31/0/1 | ✅ 一致 |
| `test_model_view` | 10/0/0 | ✅ 一致 |
| `test_fk_naming`（红线） | 6/0/0 | — |
| `drop_rate`（红线） | 1/0/0 | — |
| `test_extraction_agent`（纯度判据所在） | 26/0/0 | — |
| `test_prop_name_quality` | 8/0/0 | — |
| `test_overview` | 17/0/0 | — |

### 5.4 红线核查

| 红线 | 结果 | 证据 |
|---|---|---|
| `src/domain/view.rs` 零改动 | ✅ | `git diff --stat` **输出为空** |
| `tests/drop_rate.rs` 零改动 | ✅ | 同上为空 |
| `tests/test_fk_naming.rs` 零改动 | ✅ | 同上为空 |
| `test_humanize_still_refuses_unknown_columns` 未放宽 | ✅ | 该测试位于 **`src/domain/view.rs:1254`**（红线文件内）⇒ 零改动即天然未放宽 |
| 无测试被删除 | ✅ | 全 diff 内 `grep -E "^-.*fn test_"` **零命中**；`tests/` 侧删除行 **0** |
| 无 `#[ignore]` | ✅ | `git diff -- tests/ \| grep -E '^[-+].*#\[ignore\]'` **零命中** |
| `MODELING_TERMS` 14 词未动 | ✅ | `test_model_view_leaves_blacklist_intact` 通过；词表在 `src/domain/view.rs:897-901`（零改动文件） |
| 建模轨未写进 `view.rs` | ✅ | 新文件 `src/domain/model_view.rs`，`src/domain/mod.rs` 仅 +4 行注册 |
| `fix-props/schema.json` 绝不可动 | ✅ | 跑前跑后 **sha/size/mtime 三项全同**（见 5.5） |

### 5.5 产物 sha

命令：`./target/debug/ontok-propose --schema <根>/.review-workspace/fix-props/schema.json --out …/proposal.json --stats …/stats.json --project extracted`（`--stats` 自带路径、未传 `--names`）⇒ EXIT=0。

| 产物 | 实测 sha256 | 期望 | |
|---|---|---|---|
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | 同 | ✅ |
| `proposal.json`（`--project extracted`） | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | 同 | ✅ |

输入 `schema.json` 跑前跑后：`sha=b71c2afd…f2019`、`size=219688`、`mtime=2026-10-07 23:55:23.851232039 +0800` —— **三项全同，未被触碰**。

---

## 6 · Dev 报的 P1：独立判断

### 6.1 P1 成立吗？—— **成立，且比 Dev 说的更硬**

我独立核实到一条 Dev 与 BA 都没点明的关键事实，它把 P1 从「视角不同」升级为「**时序上根本够不着**」：

```
propose → [提案条目落在 r.payload，此时快照尚未更新]
        → review decide --track model   ← FDE 在这里裁决建模质量
        → review show（唯一能看到条目的界面）
        → proposal apply                ← src/cli.rs:648-649 门禁：!st.passed 直接拒
              └─ review.rs:333  passed 要求**每个**条目 EntryState::Passed
                 （两轨都 Accepted 才 Passed）
        → 此时才生成/更新快照
```

⇒ **FDE 裁决时，被裁决的提案内容根本还没有进任何快照。** 而本投影 `project_model_track(&Snapshot)` 读的是**已落库快照** ⇒ 它在裁决时刻能给出的，只是 **base 版本**（`src/cli.rs:607-610` 明确用的是 `row.base_version_id`）的建模情况，**不含本提案的任何一条建模细节**。

**`review show` 的输出证据**（`src/cli.rs:617-622`）：
```rust
for (r, v) in items.iter().zip(st.verdicts.iter()) {
    println!("  [{}] {:<18} {:<12} biz={:<8} model={:<8} {:?}",
        r.seq, r.target_key, r.kind, r.biz_status, r.model_status, v.state);
}
```
只打 `seq / target_key / kind / biz_status / model_status / state` —— **`payload` 一个字都没打**。而且该循环用的是原始 `items`（`ProposalItemRow`）而非 `views`，连已经构造好的 `ItemView` 都没用上。

### 6.2 BA 的补充对吗？—— **对，且可以更强**

- `src/domain/review.rs:111-118`：`payload` 字段注释确为「条目的完整候选定义（含 evidence）」，且明写「**审核状态机本身不读它**（只看 kind / target / 两端 / 双轨状态），但 `apply` 需要它」。
- `src/db/proposal.rs:262`：`to_item_views()` 确从 `r.payload.clone()` 构造 ⇒ **BA 判断成立**。

**我补一条更强的**：`payload` 里的内容**恰好就是建模轨要的那几栏**——
- `EntityPayload`（`src/extract/propose.rs:55-65`）：`key` / `evidence` / `properties[{name, column, nullable, note}]`
- `LinkPayload`（`src/extract/propose.rs:121-127`）：`from_type` / `to_type` / `cardinality` / `semantic` / `evidence`

⇒ 「BA 初判『FDE 拿不到数据』不准确」这个更正是**对的**：数据既在裁决对象上、又是建模轨正需要的形状，缺的**只是呈现**。

### 6.3 REQ-055 的范围界定有偏差吗？—— **有偏差，但不是 Dev 的偏差**

规格 §8.3 说的双轨审核是**审核提案时**的事（`review decide --track model` 裁决的就是提案条目），而 Dev 做的是**审已落库快照**（`ontology show --model-view`）。

二者是**两个不同时点、面向两种问题**的东西：
- Dev 做的：**审计/复盘**「已建的本体建模质量如何」—— 有真实价值，不是无用功；
- §8.3 要的：**裁决门禁**「这条提议该不该准」—— 这才是「客户判不了建模质量」的痛点本身，因为**门禁就在 apply 之前**。

**所以偏差出在 BA 的派单界定，不在 Dev 的执行。** BA 在 `acceptance.md` 里写下的理由是「`review.rs` 让 FDE 逐项裁决建模轨，**FDE 却拿不到建模轨的数据**」—— 这句话**准确**，但由此得出的结论「⇒ 缺的不是判据、不是数据，是**第三个投影**」**跳了一步**：FDE 在裁决时缺的不是「一个投影」，而是「把 `ItemView.payload` 里的建模细节**呈现出来**」。加第三个投影解决的是另一个（相邻但不同的）缺口。

⚠️ 公平地说，BA 的 `acceptance.md` 通篇把数据源指向**快照层**（`EntityTypeView.column`、快照层 `extracted_by`…），**从头到尾没提 `ItemView.payload`** —— 这是界定时的真实盲点。Dev 在拿到任务后也没质疑这个前提，而是照做并**主动上报了 P1**，这正是本次没有静默交付错误范围的原因。

### 6.4 修 P1 的成本多大？—— **小**（本估计基于代码结构，非实测）

最小改动就是 `src/cli.rs:617-622` 那个 `println!` 带上建模细节。两档做法：

- **最小档**：在 `ReviewCmd::Show` 的循环里追加打印 `r.payload`（数据现成，`ItemView.payload` 已克隆好，`views` 变量甚至已经在手上但没被这个循环用）。**改动面 ≈ 1 个 `println!`，不碰 `view.rs`、不碰领域层、零红线风险。**
- **完整档**：仿照 `project_model_track` 给 `payload` 做一个「提案侧建模视图」，与已落库快照的建模视图形状对齐 ⇒ FDE 在两个时点看到同一套口径。

真正的成本不在代码，而在**判据**：这会把建模术语引入 `review show` 的输出，而 `review show` 是 **FDE 轨**、不是客户轨 ⇒ 需要明确论证它**不受** `scan_modeling_terms` 约束（与 `--model-view` 同理），否则要么误伤要么得加豁免口子。**这一步是设计判断，不是编码量。**

### 6.5 P1 该并进本需求还是另开？—— **建议另开，不判 FAIL**

理由：

1. **并进去要动的东西与本需求的红线取向相反。** 本需求最硬的约束是「建模轨绝不能混进客户视图」「不许写进 `view.rs`」。修 P1 要改的是 `review show` 的**呈现面**，属于另一条通道的另一种取舍，混进来会让本需求已经干净可核的「两类型两通道」叙事变得难以核对。
2. **P1 的交付物形状不同。** 本需求交付的是「一个投影函数（纯函数、零 I/O、可单测）」；P1 的交付物是「一个 CLI 呈现 + 它的判据（建模术语在 FDE 轨的豁免论证）」。判据形态不同，混在一个需求里会让 AC 无法独立判定。
3. **本需求自身的 AC 已全部达成且证据充分**（584 全绿、10 条变异全部杀死、红线零破、产物 sha 双同）。为一个**相邻但不同**的缺口判 FAIL，会把一个已验收的改动退回重来，代价大于收益。
4. **Dev 的处理是正确且值得肯定的**：他明说「这不是我做完了」，没有把范围悄悄缩小后交付成「已完成」。这是**范围管理上的正确行为**，不是失职。

**建议动作（交 BA 决策）**：另开一条需求，建议标题「`review show` 呈现建模细节 —— 让 FDE 裁决提案时看得到 `ItemView.payload`」，并在 REQ-035 的 RP 批次登记表里把这条显式登记（避免它像 RP-2/RP-3 那样在登记表里长期「部分做」）。同时把 REQ-055 的 `status.md` 备注一句：本需求解决的是**已落库快照**的建模可审性，**未**覆盖**提案裁决时点**。

⚠️ **这不等于 P1 不重要。** 若 BA 判断 §8.3 的门禁缺口是**当前最高优先级**，那就应当**立即另开并优先**，而不是「以后再说」。P1 的真实风险是：现状下 FDE 在 apply 之前**只能闭眼点通过**，这正是 §8.3 想消灭的那个现象。**本报告不替 BA 排优先级，但明确指出该缺口的严重性未被本需求消除。**

---

## 7 · 发现但未修的问题（均非阻断）

| # | 问题 | 位置 | 判断 |
|---|---|---|---|
| F1 | `ontology show --model-view` 丢掉 `version_id` 与 `concept_groups`，且该 flag 是全 CLI 唯一原始快照 dump ⇒ **一次未声明的能力收缩** | `src/cli.rs:691-706` / `snapshot.rs:284` | 非阻断；建议在帮助文本注明「不是原始快照」 |
| F2 | 加固判据「命中含 `1:N`」**非绝对不可伪造** —— 形态守门不区分键值，字段名字面量叫 `1:N` 亦可触发 | `src/domain/view.rs:922-941` | 非阻断；有 `keeps_what_customer_view_drops` 独立兜底 |
| F3 | 「两类型 + 两通道」的**类型层**不能阻止日后把建模字段加进 `CustomerView`；真正承重的是运行时 fail-closed 守门 | `src/cli.rs:748/779/807`、`mcp/serve.rs:273` | 既有架构属性，本需求未削弱 |
| F4 | 测试内 `to_customer_input()` 是 `cli.rs::snapshot_to_view` 的**副本**，两份会漂移 | `tests/test_model_view.rs:103` | 非阻断（已在该文件 `:100-102` 注明）；已核实对本 fixture 无差异 |
| F5 | Dev 报的 M-B 杀死数 **×1** 与最终代码实测 **×2** 不符（加固后新增一条） | Dev 报告 | 非缺陷；以 ×2 为准 |

## 8 · 我自己引入又清掉的问题

| # | 事项 | 处置 |
|---|---|---|
| Q1 | 临时探针 `tests/zz_qa_probe_req055.rs`（58 行，3 个测试） | 已用 `mavis-trash` 删除，`ls` 确认不存在 |
| Q2 | 三次变异对 `src/domain/model_view.rs` 的改动 | 已从 pristine 副本还原，`sha256=35807530…ea78cb` 与改动前一致；`git diff --stat` **输出为空** |
| Q3 | 中途一次 `for t in --test test_view …` 循环因词分割误跑成 `cargo test test_view`（过滤器模式），产出大量噪音 | 已识别并以配对迭代重跑，结论以正确调用为准 |
| Q4 | 本环境无 `edit`/`write` 工具 | 登记偏离：源码变异用 `sed`、报告用带引号 heredoc（briefing 的兜底条款） |
| Q5 | 临时产物（`full-run2.log`、`interim-evidence.txt`、两份 `.pristine`、`sha/`） | 保留在指定目录 `.review-workspace/verify-req055/`；本报告为该目录唯一保留交付物 |
| Q6 | 本机无 `psql`/`psycopg` | `SELECT 1` p50 **未测**（环境不具备，非跳过） |

**收尾状态**：`git diff --stat` 为空（与 `d3ebea8` 逐字节一致）；`git status -uall` 仅剩本报告所在的 `.review-workspace/verify-req055/QA-REPORT.md`（指定交付位置）。仓库无 remote，**未 push**、未 squash、未改历史。

## 9 · VERDICT

**PASS（带非阻断项 F1–F5）**

- AC-001 ✅ 判别力成立；三条变异均**先编译通过再执行**，M-A ×3 / M-B ×2 / M-C ×4，含判别力测试本身
- AC-002 ✅ 守门零退化；隔离复跑 8 个二进制全绿，`MODELING_TERMS` 与 probe 拦截断言均在
- AC-003 ✅ 584/0/1（= 基线 574 + 新增 10，纯增量静态可证）；三条红线零改动；产物 sha 双同、输入三项未变
- P1 **属实**，但**不属本需求范围**，建议**另开**（并把严重性显式带过去）
