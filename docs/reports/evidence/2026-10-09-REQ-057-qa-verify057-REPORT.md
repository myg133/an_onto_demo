# REQ-057 QA 验收报告

- 验收对象：分支 `verify/REQ-057`，HEAD `80464a9`
- 基线：`06ae7c9`（develop，本次改动的父提交）
- worktree：`/home/myg133/Projects/Anchnet/Ontology/verify-REQ-057`
- 验收人：QA（Verifier）；本轮对 Dev 自述逐条独立复现，不采信自述数字

## 0. 结论

**PASS 带非阻断**（PASS with non-blocking findings）。

上一轮的阻断项（F-A `premise_holds` 恒真）**确实修在根上**，且是**结构性解耦**而不是打补丁：
判据不再读渲染文本、不再经过词表，**往词表加词动不了 `premise_holds` 一个 bit**（M3 实测：加词后
`premise_holds` 相关断言 0 条变红）。Dev 主动要求裁决的「改动 REQ-056 既有断言」经我独立证明是
**纯收紧**，予以接受。

残留 3 项非阻断问题（其中 1 项是我本轮新发现的判据空洞、1 项是 Dev 已报锐边的**更宽形态**）。

---

## 1. 环境与口径（先声明，避免数字歧义）

- DB 环境变量按规从仓库根 `.env` 取值并把库名 `notol` 替换为 `ontol`；口令全程只走环境变量，
  报告内无连接串、无口令。
- **本 worktree 无现成 target，全部自行编译**：无 `CARGO_TARGET_DIR`、无 `--release`、
  无 `ln -s`、未另建 probe crate。全程未与另一轮全量测试并发。
- **计数口径（全文统一）**：对 `cargo test --no-fail-fast` 输出中每一行 `test result:`
  用 `awk` 对 `$4/$6/$8` 求和（= passed / failed / ignored），并统计 `NR` = 测试二进制个数。
  **不使用 `paste -sd+`。**
- 「变异跑了多少条」一律报**测试二进制个数** + passed/failed/ignored 三元组，并注明该口径是
  全量还是子集。

---

## 2. 逐问题回答

### 问题 1：F-A 真的修在根上了吗？—— 是，且是结构性解耦

#### 2.1 判据不再碰词表（grep 证据，剥注释后）

```bash
sed 's://.*::' src/domain/review_view.rs | grep -n "scan_modeling_terms\|MODELING_TERMS"
# => 剥注释后【零命中】
```

不剥注释会命中 5 行（`src/domain/review_view.rs:27/36/41/42/349`），**全部是文档行**
（本会话已因未剥注释误报过两次，此处已剥）。即：`review_view.rs` 里**没有任何代码级词表依赖**。

签名证据：`src/domain/review_view.rs:368`
`pub fn audit_fde_channel(view: &ReviewTrackView) -> FdeChannelAudit`
—— **形参表里没有 `&str`**，结构上就无从读渲染文本。

#### 2.2 独立复现 BYPASS_E（我自己的探针 P1，与 Dev 的测试无关）

输入：建模字段一个都没有，展示名写「主键服务中心」。

```
P1 premise_holds=false hits=[]
P1 词表命中=["主键", "key"]
P1 渲染：
整体通过：是
  [1] add_entity_type · 状态=Passed
      业务轨：主键服务中心 · biz=accepted
      建模轨：key=（无 key） · model=accepted
```

⇒ `premise_holds == false`（AC-001-① 达成）。同时**词表仍然命中 `["主键","key"]`**，
这正是「解耦」的证据：不是词消失了，是判据不看词了。

#### 2.3 构造实验：把渲染输出改得面目全非，premise_holds 动不动？—— 不动

探针 P2，三路独立验证（全部实测「一致 = true」）：

| 变换 | 结果 |
| --- | --- |
| 只改 `display_target` 为塞满术语的串（渲染文本词表命中 `["外键","基数","主键","证据","key"]`） | audit 逐字一致，`premise_holds=false` |
| 把正常条目的 `display_target` 清空 | audit 逐字一致 |
| 往渲染文本末尾追加一行术语 | audit 逐字一致，`premise_holds=false` |

同时验证判据② 也不跟词表走：追加**无前缀**的术语行 ⇒ 分界违规数 `[]`；
**正控**（把建模栏真的混进业务轨行）⇒ 违规数 `1` ⇒ 证明上一步不是恒真。

#### 2.4 独立重跑词表实验（M3：往 `MODELING_TERMS_CJK` 加「客户」）

口径：全量 `cargo test --no-fail-fast`，44 个测试二进制
⇒ **591 passed / 19 failed / 1 ignored**。

- **`premise_holds` 相关断言变红数 = 0**（已逐条查红条清单与 panic 行号核实）。
- 19 条红条的构成：
  - **18 条 = 客户视图侧词表守门**（`tests/test_view.rs`、`tests/test_mcp_server.rs`、
    `tests/test_model_view.rs`、`tests/test_overview.rs`、`tests/test_concept_transition.rs`、
    `tests/test_review_view.rs`、`tests/test_review_show_e2e.rs`），
    典型 panic 文案如「客户视图泄露了建模术语：["客户"]」
    （`tests/test_model_view.rs:192`）、「业务轨呈现泄露建模术语：["客户"]」
    （`tests/test_review_show_e2e.rs:229`）。
  - **1 条 = 环境类**：`tests/test_provenance_source.rs:180`
    「✗ error communicating with database: Connection reset by peer (os error 104)」
    —— 该文件本需求**零改动**；隔离复跑 **3 次全部通过**（3/3，16~18 s）。归环境类，不计代码失败。

- ⚠️ **口径差异（非虚报）**：Dev 报「4 条全在客户视图侧」。我的全量口径是 18 条客户侧。
  Dev 的 4 条大概率只跑了若干个二进制。**方向性结论（客户侧变严、FDE 侧不受影响）与我一致**。

#### 2.5 我构造的新绕过：空串 / 空白 / 嵌套 —— key 侧全部堵住，evidence 侧漏了

探针 P3，23 种退化形态，17 种符合预期、**6 种不符合**：

| 形态 | `premise_holds` | 判定 |
| --- | --- | --- |
| `key` = `""` / `"   "` / `42` / `null` / `true` | `false` | ✅ 符合 |
| `key` = 嵌套对象 `{"a":1}` / 嵌套数组 `["customer"]` | `false` | ✅ 符合（`as_str()` 失败即 None） |
| `properties[].name` = `""` / `"  "` / 非串 / 空数组 | `false` | ✅ 符合 |
| `evidence` = `[]` | `false` | ✅ 符合 |
| `cardinality` = `""` / `"   "` / 非串 | `false` | ✅ 符合 |
| `from_to` 单端 | `false` | ✅ 符合 |
| **`evidence` = `[""]` / `["   "]` / `[{}]` / `[null]` / `[[]]` / `[0]`** | **`true`** | ❌ **缺口** |

缺口机制（代码阅读 + 实测吻合）：`render_evidence_entry`
（`src/domain/review_view.rs:631-651`）认不出 `{table,column,claim}` 三段式时**退回
`v.to_string()`**，得到 `{}` / `""` / `null` / `[]` / `0` 这些**非空字符串**；
`sourced_fields`（`src/domain/review_view.rs:198-224`）判的是
`self.evidence.iter().any(|e| !e.trim().is_empty())` —— 判的是**渲染后的串**，
于是这些退化元素被算成「来自 payload 且非空」。

探针 P7 补测「FDE 看不看得见」：渲染行分别是
`建模轨：key=（无 key） · 证据：{} · model=accepted` / `证据：""` / `证据：null` / `证据：0`，
分界判据**零违规**。⇒ 缺失/退化在输出里**可见**，但**自检不会报警**。

**判级：非阻断（中低）。** 与原阻断项**不同类**——原问题是「任何非空提案恒真」，
本项是「`evidence` 数组里有一个退化元素」，且该数组必须来自 payload。
建议（不阻断本轮验收）：把 `sourced_fields` 对 evidence 的判据从「渲染串非空」
改成「至少有一项认得出结构或至少有一个非空字符串字段」。

### 问题 2：Dev 改的那条既有断言该接受吗？—— **接受**（附流程意见）

被改的断言在 `tests/test_review_view.rs:481-486`，属于 REQ-056 的
`test_review_view_link_ends_are_keys_not_display_names`（`tests/test_review_view.rs:461`）。

- 旧：`!link.model.type_key.contains("客户")`
- 新：`assert_eq!(link.model.type_key.as_deref(), Some("customer_has_subscription"))`

#### 2.6 松紧关系：证明「纯收紧」成立

**集合包含论证（比抽样更强）**：
`new_pass ⟹ type_key == "customer_has_subscription" ⟹ ¬contains("客户") ⟹ old_pass`。
故 `Pass(新) ⊆ Pass(旧)` ⇒ **不存在**「旧会抓、新放过」的输入 ⇒ 新断言通过集是旧断言通过集的
**真子集**（严格更小 ⇒ 严格更严）。

**实证（探针 P5，51 个样本：11 个具名 + 40 个穷举组合）**：

- 「旧过新挂」（收紧证据）**6 个**：`customer`、`subscription`、`主键服务中心`、`""`、`"  "`、
  `CustomerHasSubscription`
- 「新过旧挂」（放松证据）**0 个**

其中 `主键服务中心` 正是 Dev 说的 BYPASS_E 展示名：**旧断言会放过它**（不含「客户」），
**新断言抓住它**。Dev 的说法成立。

#### 2.7 流程上该由谁裁决

- **裁决权在 QA**（谁判 FAIL 谁判这次改动），这与 Dev 主动上报、我独立举证一致。
- 但流程上有一个**未闭环的缺口**：`BA/demands/REQ-057/` 下**只有 `acceptance.md`**，
  其中**没有任何关于「改动 REQ-056 既有断言」的记录**。
  `acceptance.md:5` 写的是「本需求只修这两条，不得顺手改别的」。
- **处理建议（BA 动作，不阻断代码验收）**：在 REQ-057 的验收记录里补一条
  「REQ-056 测试断言变更登记」，写明：改了哪条、旧→新、为何是收紧、QA 裁决结论、
  以及「改上一轮需求的测试必须由 QA 裁决并登记，不得静默修改」的流程条款。
- 我的态度：**代码层面接受；流程层面要求补登记**。

### 问题 3：F-D 修法对吗？—— 对

#### 2.8 `ModelFacing::type_key` 真的只从 `payload["key"]` 取

`src/domain/review_view.rs:596-598`：

```rust
fn payload_key(payload: &Value) -> Option<String> {
    payload_str(payload, "key").map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}
```

投影处 `src/domain/review_view.rs:290` 为 `type_key: payload_key(&it.payload)`，
**没有任何 `it.target` 参与**。字段类型已改为 `Option<String>`
（`src/domain/review_view.rs:168`）⇒ 「没有 key」在**类型上**就是 `None`，
不是靠字符串哨兵。渲染缺 key 时打 `key=（无 key）`（`KEY_MISSING_MARK`，
`src/domain/review_view.rs:492`；渲染逻辑 `src/domain/review_view.rs:524-529`）。

#### 2.9 `ItemView::type_key()` 的回退原样保留（红线文件）

`src/domain/review.rs:133-139` 逐字未动（`git diff --numstat 06ae7c9 80464a9 -- src/domain/review.rs` = 0 行）。
探针 P6 实测回退仍在：

```
P6 缺key时 ItemView::type_key() = "主键服务中心"   ← 回退展示名仍在
P6 空白key时 ItemView::type_key() = "  "           ← 不 trim、从不进回退分支
```

**我独立同意 Dev 的判断**：那个回退对 `summarize`/`apply` 是对的 ——
`src/domain/review.rs:122-132` 的注释说明了原因（`available` 存 key 命名空间，
关系两端也用 key 查；若此处不回退，缺 key 的条目会把展示名塞进 key 命名空间，
两端按 key 查就必然悬空）。F-D 只在**展示侧**另开一路，是正确的分层。
同时确认 `ItemView.from_type` 确实取自 payload（`src/db/proposal.rs:250-252`），
所以 `sourced_fields` 把 `from_to` 称作「来自 payload」措辞站得住。

#### 2.10 四种形态实测渲染（探针 P4）

| 形态 | `type_key` | 建模轨前两段 |
| --- | --- | --- |
| 缺 `key` | `None` | `建模轨：key=（无 key） · model=accepted` |
| `key` = `""` | `None` | 同上 |
| `key` = `"  "` | `None` | 同上 |
| `key` = `42` | `None` | 同上 |
| `key` = `{"a":1}` | `None` | 同上 |
| `key` = `"  customer  "` | `Some("customer")` | `建模轨：key=customer · model=accepted`（**被 trim**） |
| `key` = `"主键服务中心"`（payload 里就写了展示名） | `Some("主键服务中心")` | `建模轨：key=主键服务中心 · model=accepted` |

**哪个形态仍会「展示名冒充 key」？** 只有最后一种：**payload.key 字段本身就是展示名**。
这不是 F-D 的失效——F-D 的契约是「只从 payload 取」，而判据无法区分
「payload.key 里放的是 key」与「payload.key 里放的是展示名」。
生产侧 payload 由 `src/extract/propose.rs` / `src/agent/assemble.rs` 生成，
写入的是 `l.key`（系统 key），所以现实路径上不会发生。记为**边界**，非缺陷。

### 问题 4：变异表核实（见第 4 节）

### 问题 5：全量与红线（见第 5 节）

---

## 3. AC 逐条对照

| AC | 判定 | 依据 |
| --- | --- | --- |
| AC-001 判据落在字段溯源上 | ✅ 通过 | P1 复现 false；P2 三路解耦；P3 key 侧 17/23 符合 |
| AC-002 type_key 回退不冒充 key | ✅ 通过 | P4 七形态逐字；`review.rs` 零改动 |
| AC-003 BYPASS_A/B/C 补判据 | ✅ 通过 | 新增判据② `verify_track_separation`（`review_view.rs:405`）四条规则 + A/B/C/漏打用例 |
| AC-004 全量与红线 | ✅ 通过 | 见第 5 节 |

AC-003 授权了「补判据」这件事，所以 `cli.rs` 里新增 `verify_track_separation` 接线**不属越界**
（虽然 `acceptance.md:5` 说「只修这两条」，但 AC-003 明确要求补判据 —— 以 AC-003 为准）。

---

## 4. 变异测试表

口径：除 M2C 外均为**全量** `cargo test --no-fail-fast`（44 个测试二进制）；
M2C 只跑了 2 个相关二进制，已单独标注。**编译失败一律不算变异执行**——
本表三条 Dev 变异 + 我加的一条，**全部编译通过并真实执行**。

| 变异 | 做法 | 运行条数（口径） | 结果 | 杀死它的判据（测试名） |
| --- | --- | --- | --- | --- |
| **M1** 旧词扫描判据 | `audit_fde_channel` 体改回 `scan_modeling_terms(&render_review_text(view))` | 44 个二进制 / **602 passed / 8 failed / 1 ignored** | 🔴 红 8 条 | `test_review_view` 二进制 6 条：`bypass_e_premise_false_even_when_terms_present_in_text`、`fde_exemption_is_conditional_on_a_checkable_property`、`premise_still_holds_for_normal_items`、`hits_are_field_provenance_not_wordlist`、`empty_key_alone_does_not_hold_premise`、`bypass_b_evidence_only_never_shows_display_name_as_key`；`test_review_show_e2e` 二进制 2 条：`carries_modeling_fields`、`missing_key_never_becomes_display_name` |
| **M2B** 忠实 `type_key()` 回退 | `type_key: payload_key(&it.payload)` → `type_key: Some(it.type_key())` | 44 个二进制 / **605 passed / 5 failed / 1 ignored** | 🔴 红 5 条 | `test_review_view` 4 条：`missing_key_never_renders_display_name_as_key`、`bypass_b_...`、`bypass_e_...`、`fde_exemption_is_conditional_...`；`test_review_show_e2e` 1 条：`missing_key_never_becomes_display_name` |
| **M2C**（我加的互补变异）只去掉空白硬化、保留「不回退展示名」 | `payload_key` 去掉 `trim`+`filter`；渲染去掉 `str::trim` | **仅 2 个二进制**（`test_review_view` + `test_review_show_e2e`）/ **25 passed / 1 failed** | 🔴 红 1 条 | `test_review_view_blank_key_does_not_fall_back_to_display_name`（**唯一杀手**） |
| **M3** 词表加宽 | `src/domain/view.rs:904` `MODELING_TERMS_CJK` 加 `"客户"`（红线文件，跑完已 sha256 复原） | 44 个二进制 / **591 passed / 19 failed / 1 ignored** | 🔴 红 19 条（18 客户侧 + 1 环境类） | 18 条客户侧词表守门用例（见 2.4）；**FDE 侧 `premise_holds` 断言 0 条变红** |

**存活变异论证（Dev 主动标注的那条，我独立核实成立）**

`test_review_view_blank_key_does_not_fall_back_to_display_name` 在 **M2B 下存活**（已 grep 到
`... ok`）。原因与 Dev 自述一致：原 `ItemView::type_key()` 对 `"  "` **不 trim**、因 key 字段存在
**从不进回退分支**，返回 `"  "`；渲染侧再 trim 成空 ⇒ 仍打「（无 key）」⇒ 该测试的两条断言都成立。

**但「存活」不等于「空转」**：我为此专门设计并执行了 M2C（去掉空白硬化、保留不回退展示名），
结果是 **该测试是唯一红条**。⇒ 它对「空白 key 不硬化」这条性质**有且仅有**判别力，
不是凑数测试。Dev 的诚实标注成立，且该测试应当保留。

**Dev 报数与我实测的口径差异（均非虚报，只是范围不同）**：

- M1：Dev「6 挂」= 只数了 `test_review_view` 那 6 条；我全量口径 8 条（多出 e2e 的 2 条）。
- M2B：Dev「4 挂」= 同上；我全量口径 5 条。
- M3：Dev「4 条全客户侧」；我全量口径 **18 条客户侧**（+1 环境类）。

---

## 5. 测试与环境 / 红线 / 产物 sha

### 5.1 全量与基线对照

| 项 | passed | failed | ignored | 测试二进制数 |
| --- | --- | --- | --- | --- |
| 基线 `06ae7c9` | **596** | **0** | **1** | 44 |
| HEAD `80464a9` | **610** | **0** | **1** | 44 |
| delta | **+14** | 0 | 0 | 0 |

+14 的归属（口径：文件内测试函数计数 + 二进制内实际条数双向印证）：

- `tests/test_review_view.rs`：7 → 19（**+12**）
- `tests/test_review_show_e2e.rs`：5 → 7（**+2**）

那条 `ignored` 是 `tests/test_mcp_server.rs` 的 `todo_identifier_defenses_have_own_assertions`，
**基线与 HEAD 都在**（pre-existing，非本需求引入）。

### 5.2 红线核查

- 整仓 `git diff --name-only 06ae7c9 80464a9` 只有 4 个文件：
  `src/cli.rs`、`src/domain/review_view.rs`、`tests/test_review_show_e2e.rs`、`tests/test_review_view.rs`。
- 红线四文件 `git diff --numstat 06ae7c9 80464a9 -- <file> | wc -l` **全为 0**：
  `src/domain/view.rs`、`src/domain/review.rs`、`tests/drop_rate.rs`、`tests/test_fk_naming.rs`。
  （⚠️ 注意 `drop_rate.rs` 的真实路径是 **`tests/`**，不是 `src/domain/`。）
- `test_humanize_still_refuses_unknown_columns` 位于 **`src/domain/view.rs:1254`**
  （在红线文件里）。`diff <(git show 06ae7c9:src/domain/view.rs) <(git show 80464a9:src/domain/view.rs)`
  **输出为空 ⇒ 整文件逐字相同 ⇒ 断言一条都没放宽**。它仍是 `#[test]`，未被 ignore。
- 无测试被删除：`comm -23` 对比基线与 HEAD 的测试函数名，两文件均**空**。
- 无新增 `#[ignore]`、无删除 `#[ignore]`（对 diff 全量 grep）。
- 客户视图的建模术语守门：`view.rs` 零改动 ⇒ **一条未动**。

### 5.3 产物 sha（clean HEAD 复算，两个都逐字命中）

命令（**未传 `--names`**，`--stats` 自带路径参数；输入即仓库根
`/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`，**只读引用，未改动**）：

```bash
cargo run --bin ontok-propose -- \
  --schema /home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json \
  --out .review-workspace/verify-req057/proposal.json \
  --stats .review-workspace/verify-req057/stats.json \
  --project extracted
```

| 产物 | 期望 | 实测 | 判定 |
| --- | --- | --- | --- |
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | 同左 | ✅ |
| `proposal.json` | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | 同左 | ✅ |

### 5.4 环境类失败（与代码分开写）

- `tests/test_provenance_source.rs::test_concept_import_also_marks_extracted_by_as_declared`
  在 **M3 那一轮**出现 `Connection reset by peer (os error 104)`。
  该文件本需求零改动；**隔离复跑 3 次全部通过**（16.08s / 18.08s / 16.90s）。
  **归环境类**，不计入代码失败。复现率随共享库负载浮动，**我不声称稳定复现率**。
- clean HEAD 的两轮全量（第 1 轮）与最终 smoke 均**无此失败**。
- 本机无 `psql`、无 `psycopg` ⇒ 未测 `SELECT 1` p50（与要求一致，不臆造该数字）。
- 共享本体库全程**只读使用**（只跑既有测试套件），我**没有**执行任何 DELETE / TRUNCATE / 清表。

---

## 6. 发现但未修的问题（Dev 已知或我新发现）

| # | 严重度 | 问题 | 触发条件 | 影响 | 证据 |
| --- | --- | --- | --- | --- | --- |
| F-1 | **中低（非阻断）** | `evidence` 栏的结构性退化元素被算作「来自 payload 且非空」⇒ `premise_holds=true` | `payload.evidence` 数组含**任一**认不出 `{table,column,claim}` 三段式的元素：`[""]`/`["   "]`/`[{}]`/`[null]`/`[[]]`/`[0]` | 自检对退化输入不报警；FDE 在渲染里能看到 `证据：{}` 但拿不到告警 | 探针 P3/P7；机制在 `review_view.rs:631-651` 与 `review_view.rs:198-224` |
| F-2 | **低（Dev 已报，我扩测）** | 判据② 前缀锐边**比 Dev 报的更宽** | 业务/载荷文本里出现**字面量**「业务轨：」或「建模轨：」 | `review show` 向 stderr 打出**误报**；不影响数据正确性 | Dev 只报了 `display_target` 含「业务轨：」；我实测：展示名含「建模轨：」⇒ **4 条误报**；**证据 claim 含「业务轨：」⇒ 2 条误报**；列名含「建模轨：」⇒ 0 条。成因：规则①②用 `contains` 数行 + 规则②用 `trim_start_matches`（会**连续**剥掉重复前缀） |
| F-3 | **信息（边界，非缺陷）** | payload.key 字段本身是展示名时仍渲染 `key=主键服务中心` | 生产侧若把展示名写进 `payload.key` | FDE 可能把展示名当系统标识 | 探针 P4 最后一行；契约是「只认 payload」，判据无法区分 |
| F-4 | **信息** | 投影层 trim、而红线 `review.rs` 的 `type_key()` 不 trim ⇒ 两层看到的 key 可能不同 | `payload.key` 带前后空白（如 `"  customer  "`） | 建模轨显示 `key=customer`，而 `summarize`/`apply` 看到的是带空白的原值 | 代码阅读结论（`review_view.rs:597` vs `review.rs:134-138`）；**未做运行期后果验证**；`review.rs` 是红线，本需求不应动 |

---

## 7. 我自己引入又清掉的问题（如实报）

1. **探针 P2 初版断言写错，导致 `qa_p2` 假红**：我断言「往渲染追加无前缀术语行应产生分界违规」。
   实际上无前缀的额外行**不违反任何规则**，零违规才是正确行为。判定为**我的探针 bug，
   不是产品缺陷**。已改正断言并**补加正控**（真实混轨 ⇒ 违规数 1），证明改后不是恒真。
2. **红线核查第一轮路径写错**：把 `drop_rate.rs` 写成 `src/domain/drop_rate.rs`（该路径不存在），
   导致那次 `diff --stat` 静默返回空、看起来「全 0」实则**该文件根本没被检查**。
   已按真实路径 `tests/drop_rate.rs` 重做（numstat 行数 0）。
3. **`edit` / `write` 工具在本会话不可用**（函数清单里没有这两个工具），
   `edit` 对含中文的长行连续 3 次匹配失败。按交付纪律的兜底条款，改用**带引号 heredoc**
   写 Markdown / 用 `python3` 精确替换，并在此登记偏离。
4. **变异期间的临时改动已全部复原并逐文件 sha256 校验**：`src/domain/view.rs`
   （M3 改过红线文件，sha `c0c2df2c…` → `56653a0f…` → **复原 `c0c2df2c…` 成功**）、
   `src/domain/review_view.rs`（M1/M2B/M2C 三次改动 → 复原 `30c29f46…` 成功）。
5. **基线对照需要临时 checkout `06ae7c9`**，已用 `git checkout 80464a9 -- .` + `git reset`
   复原，`git diff HEAD --stat` **为空**。

## 8. 偏离登记

- 未使用 `CARGO_TARGET_DIR` / `--release` / 另建 probe crate / `ln -s` 指向别的 target。
- 未与另一轮全量测试并发。
- 求和未使用 `paste -sd+`（用 awk）。
- 临时产物只在 `.review-workspace/verify-req057/`（含本报告）；未用 `/tmp`、未用 home。
- 探针 Rust 测试放在 `tests/zz_qa_probe_req057.rs`（放这里才参与编译），**收尾已用 `mavis-trash` 回收**。
- 仓库无 remote，**未 push、未 squash、未 force、未改历史**。
- DB 口令只走环境变量，报告内无连接串。

## 9. 收尾状态

```
git rev-parse --short HEAD   => 80464a9
git diff HEAD --stat         => (空)
git status -uall --porcelain => 仅 ?? .review-workspace/verify-req057/ 下的验收产物
```

7 个关键文件 sha256 与验收开始时**逐一比对成功**（`sha256sum -c` 全「成功」）。
最终 smoke：`test_review_view` 19 passed / 0 failed；`test_review_show_e2e` 7 passed / 0 failed。

---

## VERDICT

**PASS 带非阻断** —— 阻断项已结构性修复并被独立复现与变异验证；残留 4 项均为非阻断，
其中 F-1 建议下一轮跟进，F-2 的更宽形态建议 Dev 知悉。
