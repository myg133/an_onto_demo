# REQ-060 验收报告（QA 独立核实）

- **worktree**：`/home/myg133/Projects/Anchnet/Ontology/verify-REQ-060`
- **分支 / HEAD**：`verify/REQ-060` / `ca8cdc7cad6c09445ea8fd9649650fa76ea19bc6`
- **基线**：`f90d537`（develop）
- **报告路径偏离登记**：简报要求写进 worktree 内的 `.review-workspace/verify-req060/`，但本 worktree 分支的 `.gitignore` 只含 `/target`、`.env`、`out/`，**不忽略** `.review-workspace`，写进去会让 `git status -uall` 非空，与硬性纪律冲突。故改写到**仓库根** `.review-workspace/verify-req060/`（与 Dev 的 `req060/` 同级、同一约定）。代价：偏离简报一处路径，收益：`git status -uall` 严格为空。

---

## 1. 结论

**PASS 带非阻断（3 项非阻断发现，其中 1 项是存活变异）。**

AC-001 ~ AC-005 全部成立，四条红线零改动，产物 sha 与基线**逐字节一致**。
Dev 用来避开 BA 点名坑的「类型层手段」**是真的牢** —— 不是靠约定，是靠类型与数据流，构造证明成立。
唯一需要 BA 决策的是 **F-2（存活变异 M2b）**：判据③ 声称能管「归属行的位置」，实际管不到「归属**只在某一个条目头行里、用分隔符隔开**」这种形态。它不违反 AC-002（提案级措辞仍在），但判据文档承诺大于实现。

---

## 2. 逐问题回答

### 2.1 类型层解耦真的成立吗？—— 成立，且比 Dev 声称的更硬

**(a) `provenance` 确实在 `ReviewTrackView` 而不在 `ModelFacing`**

- `src/domain/review_view.rs:140` — `pub provenance: ProposalProvenanceView,`（`ReviewTrackView` 结构体字段区，该结构体 `:120` 起、`:141` 结束）
- `src/domain/review_view.rs:413-441` — `ModelFacing` 字段全集：`type_key / columns / evidence / cardinality / from_type / to_type / status`。**无 `provenance`、无任何归属字段。**

裸 `grep` 证据（对 `ModelFacing` 定义区 413-441 搜 `provenance|agent|Provenance`）：**0 命中**。

**(b) 构造证明（不是「我读了所以是这样」）**

Rust 结构体字面量要求字段集**穷尽**。我的临时探针里写了

```rust
let mf = ModelFacing {
    type_key: None, columns: vec![], evidence: vec![],
    cardinality: None, from_type: None, to_type: None, status: Decision::Pending,
};
```

**编过** ⇒ 这 7 个就是字段全集。归属若进不了这个结构体，就进不了 `sourced_fields()` 的 `&self`。

**(c) `sourced_fields()` 的接收者看不到 `provenance`**

- `src/domain/review_view.rs:460` — `pub fn sourced_fields(&self) -> Vec<&'static str>`，接收者 `&ModelFacing`
- 它只 `self.` 这 6 个字段：`type_key / columns / evidence / cardinality / from_type / to_type`（对 460-480 行 grep `self\.[a-z_]*` 去重所得）
- 返回值是 `Vec<&'static str>` 的**字面量名**，不是字段值 —— 所以「返回里出现归属」在类型上就不可能，除非有人改这个函数体（= 变异 M8，见 §3）

**逻辑链闭合**：归属不在 `ModelFacing` ⇒ 进不了 `&self` ⇒ 进不了返回值 ⇒ `audit_fde_channel`（`:686-694`，只做 `it.model.sourced_fields()` 并拼 `item[seq].field`）的 `hits` 里不可能有归属 ⇒ `premise_holds = !hits.is_empty()` 与归属无关。

**(d) 独立复现 Dev 的实验**

我自己造了一份**连 `key` 都没有**的条目（`payload` 只有 `{"display_name":"裸条目"}`），有/无归属两种输入：

```
PROBE2 无归属 = FdeChannelAudit { premise_holds: false, hits: [] }
PROBE2 有归属 = FdeChannelAudit { premise_holds: false, hits: [] }
```

**逐 bit 相同**（`premise_holds` 与 `hits` 两个都比了），与 Dev 自报一致。

**(e) ⚠️ 独立构造绕过：把归属内容塞进 `ModelFacing.evidence`**

我用**中性文本**（`抽取者：mcode-agent · 版本 2026.10.07`，刻意**不含**归属行前缀，以免判据③「靠前缀计数」这条偶然路径掩盖结论）注入 `evidence`：

```
PROBE7 注入前 = FdeChannelAudit { premise_holds: false, hits: [] }
PROBE7 注入后 = FdeChannelAudit { premise_holds: true,  hits: ["item[1].evidence"] }
PROBE7 判据③ 注入前=0 条 / 注入后=0 条
PROBE7 判据② 注入前=0 条 / 注入后=0 条
```

**结论（必须讲清的两件事）**：
1. `premise_holds` **确实会变真** —— AC 点名的那个坑在「内容注入」这个变体下是真的会发生的。
2. **两条运行时判据都抓不住**（各报 0 条违规）。抓住它的是**单元测试**，不是运行时判据。我做了源码级变异 M1 证实（§3）：`test_review_provenance_premise_holds_is_bit_identical_with_and_without_provenance` 与 `test_review_show_e2e_premise_holds_unaffected_by_provenance` 双双变红，报 `item[1].evidence`。

⇒ **评价**：Dev 的「类型层手段」防住的是「**加字段**」这个动作（编译期就挡住）；防不住「**搬运已有文本**」。这个缺口由测试兜住了，是可接受的分工，但**报告里必须写明「结构性保证的边界在哪」**，否则下一个 Dev 可能误以为运行时判据也能兜。

---

### 2.2 四态真的分得开吗？—— 分得开

**(a) 四态逐字输出（我自己跑的 `render_review_text`，非 Dev 日志）**

```
[Declared]
提案级抽取归属：agent=mcode-agent · version=2026.10.07 · 第 1 轮（整份提案共用一份，不是逐条目的）

[HumanRevised / SQL NULL]
提案级抽取归属：无（人工修订，ADR-001：这份提案的 extracted_by 是 SQL NULL）（整份提案共用一份，不是逐条目的）

[NotProvided]
提案级抽取归属：未提供（这条路径没有接提案行，无法判断谁抽的 —— **不是**「人工修订」）（整份提案共用一份，不是逐条目的）

[Suspected / agent 空串]
提案级抽取归属：⚠ 形态可疑，不作为归属采信 —— `agent` 是空串；原始取值：{"agent":""}（整份提案共用一份，不是逐条目的）
```

四行**两两不同**（探针用 `BTreeSet` 断言 `len()==4`）。

**(b) `NotProvided` 的必要性 —— Dev 的理由成立，但「今天会被用到」不成立**

Dev 说「两参入口压根没收归属，显示成人工修订 = 假安心」。前半句**对**，后半句是**假想**：

| 入口 | `src/` 里的生产调用方 |
|---|---|
| `project_review_track_with_provenance`（三参） | **有** —— `src/cli.rs:636`（`review show` 唯一入口） |
| `project_review_track`（两参） | **无** |
| `project_review_track_with_summary`（两参包装） | **无** |

`grep -rn "project_review_track" src/` 的全部结果：只有 `cli.rs:636` 与 `review_view.rs` 自身定义。
另外我查了 `ReviewTrackView` 的持久化点：**一个都没有**（`src/` 里除 `cli.rs:636` 外无引用，无 `INSERT`/`to_value`），所以 serde 那条 `#[serde(default)]` 兜底路径（我探针 PROBE6 实测：缺 `provenance` 字段反序列化 ⇒ 确实落 `NotProvided`）**在生产上不可达**。

⇒ **独立判断**：`NotProvided` 今天是为**未来**准备的类型级护栏 + `pub` API 契约（`tests/test_review_view.rs` 有 4 处两参调用，是活代码），不是为某个现存生产路径准备的。**这不是缺陷** —— 把「没接通」表示成「有人审过」在类型上不可表达，本身就是对的护栏；只是它的收益目前是**预防性的**，Dev 的措辞略微夸大了现状。

**(c) 六种可疑形态的原因真的互不相同吗？—— 真的不同，不是换措辞**

Dev 的 6 种我全复现；我另外加了 3 种输入（`agent: null`、整体是数组、整体是布尔）→ **9 种输入 → 9 种互不相同的 reason**：

```
agent 空串                => `agent` 是空串
纯空白 agent              => `agent` 只有空白
缺 agent                  => 缺 `agent` 栏（没有它就无法回答「谁抽的」）
agent 非字符串(数字)      => `agent` 不是字符串（实际是 数字）
agent 非字符串(null)      => `agent` 不是字符串（实际是 null）
整体非对象(字符串)         => extracted_by 不是对象（实际是 字符串）
整体非对象(数组)           => extracted_by 不是对象（实际是 数组）
整体非对象(布尔)           => extracted_by 不是对象（实际是 布尔）
JSON null                 => extracted_by 是 JSON null（不是人工修订的 SQL NULL）
```

前 8 条**语义各不相同**（缺失 / 类型错 / 空白 / 非对象类型 / JSON null），不是同一句话换词。**只有**「整体非对象」那 3 条共享同一句式、靠 `type_name()` 区分具体类型 —— 这是合理的（它们确实是同一类缺陷），但严格说 9 条里只有 **6 类**独立原因。Dev 说「六种」是准确的。

**(d) ⚠️ JSON null vs SQL NULL —— 分得开**

| 输入 | 状态 | 输出 |
|---|---|---|
| `None`（SQL NULL） | `HumanRevised` | `无（人工修订，ADR-001：这份提案的 extracted_by 是 SQL NULL）` |
| `Some(Value::Null)`（JSON null） | `Suspected` | `⚠ 形态可疑，不作为归属采信 —— extracted_by 是 JSON null（不是人工修订的 SQL NULL）；原始取值：null` |

分得开，且**两行输出不同**。变异 M4（把 JSON null 改回 `HumanRevised`）被 `test_review_provenance_suspicious_shapes_are_labelled` 杀掉 ⇒ 这个区分**有测试护栏**，不是靠自觉。

**一个小观察（非缺陷）**：JSON null 那一行的文案里含「人工修订」四字（语境是「**不是**人工修订的 SQL NULL」）。人读没问题，但 `verify_provenance_scope` 的规则③是靠子串 `人工修订` 判「有表态」的 —— 也就是说，将来若把措辞改得只剩「不是人工修订的 SQL NULL」，规则③对 `HumanRevised` 仍是零命中，无副作用。**记一笔，属措辞层面，无行动项。**

---

### 2.3 `verify_provenance_scope` 有判别力吗？—— 有，但有一个明确的覆盖缺口

**实现位置**：`src/domain/review_view.rs:813-871`，三条规则：① 归属行恰好 1 行；② 不带两轨前缀且排在第一个条目行之前；③ 四态各自必须打出判定词。

**(a) Dev 自报的三个变异 —— 我独立重跑（每条给二进制名 + 运行条数 + 红了几条）**

| 变异 | 运行二进制（运行条数） | 结果 | 红了几条 |
|---|---|---|---|
| M7 逐条目重复打印（= Dev 变异 2） | `test_review_provenance`(14) / `test_review_show_e2e`(11) / `test_review_view`(19) | 被杀 | **5** |
| M6 可疑形态照打成正常归属（= Dev 变异 3） | 同上 | 被杀 | **2** |
| M8 `sourced_fields` 里加 `"provenance"`（= Dev 变异 1） | 同上 | 被杀 | **8** |

口径：`cargo test --test A --test B --test C --no-fail-fast`，红条数 = 三个 `test result:` 行的 failed 字段求和。日志：`mut-M7.log` / `mut-M6.log` / `mut-M8.log`。

**与 Dev 自报的一处出入（如实记）**：Dev 说变异 1「**只有**结构性那条断言抓得住，`premise_holds` 那条没抓到」。我构造的 M8（`sourced_fields` 无条件 `push("provenance")`）让 `hits` 恒非空 ⇒ `premise_holds` 恒真 ⇒ **`premise_holds` 那两条也红了**（`premise_holds_is_bit_identical…`、`e2e_premise_holds_unaffected…`）。差异应来自 Dev 当时的具体写法（他称之为「弱代理版」）。**结论不变（被杀），但 Dev 对「哪条判据抓得住」的归因不准。**

**(b) 独立构造的别的绕过（判据层，用手工构造的畸形文本喂 `verify_provenance_scope`，不改源码）**

| 畸形文本 | 判据③违规条数 |
|---|---|
| A 逐条目重复（4 行） | 1 —— `应恰好 1 行` ✅ 抓 |
| B **只打 `agent=` 不打 `version=`** | **0** ⚠️ 不抓 |
| C 归属放进节标题行、**去掉**归属行前缀 | 1 —— `行数为 0` ✅ 抓 |
| C2 归属放进节标题行、**保留**归属行前缀 | **0** ⚠️ 不抓 |
| D 归属打在第 1、2 两个条目之间 | 1 —— `排在条目行之后` ✅ 抓 |
| **E 归属用 ` · ` 分隔符隔在 `[1]` 条目头行内（保留前缀与措辞）** | **0** ⚠️ 不抓 |

**(c) 那条判据判的是「不在条目内」，能不能防住「在条目内但用分隔符隔开」？—— 不能，已用源码级变异坐实**

变异 **M2b**：把归属行从「独立行」改成**只拼进第 1 个条目的头行**（`  [1] add_entity_type · 状态=Passed · 提案级抽取归属：…`），并把独立归属行删掉。

```
test_review_provenance:   14 passed; 0 failed
test_review_show_e2e:     11 passed; 0 failed
test_review_view:         19 passed; 0 failed
```

**44 条全绿 —— 存活变异。** 判据③ 报 0 条，判据② 报 0 条，端到端那条「归属恰好 1 行」的计数也照样通过（确实只有 1 行）。

**为什么抓不住**：规则②只检查两件事 —— (i) 该行是否含两轨前缀；(ii) 该行下标是否晚于**第一个**条目行。`[N] … 状态=` 头行**既不含两轨前缀、又排在第一个条目头行上**，两个条件都躲过去了。判据**没有**「该行不得是条目头行」这一条。

**(d) 存活变异的论证与影响面**

- **不违反 AC-002**：`PROVENANCE_SCOPE_NOTE`（「整份提案共用一份，不是逐条目的」）仍在该行里，FDE 读到的仍然是「整份提案的归属」，不是「条目 1 的归属」。所以**产品行为是对的**。
- 我把绕过推到最狠做了 **M2c**（M2b + 把「提案级」措辞一起删掉）：**被杀**，1 条红 —— `test_review_provenance_wording_says_proposal_level`。⇒ **AC-002 点名的那个误导形态（归属看不出是提案级）确实被守住了**，守它的是措辞断言，不是判据③。
- ⇒ 定性：**判据③ 的文档承诺（「抓：塞进某一条的行 / 排到条目区里」，见 `:807-809`）大于它的实现**。属于**非阻断的判据覆盖缺口**，建议 BA 决定是否开后续小条。

---

### 2.4 Dev 的两处失误真修了吗？

**(1) 那条「方向反了」的断言 —— 真修好了，现在是双向的，且我独立验证了方向**

代码位置 `tests/test_review_provenance.rs:261-303`，两条方向相反的断言并存：

- **正向（正确标注可疑 ⇒ 放行）**：`:268-272` `assert!(verify_provenance_scope(&view, &text).is_empty(), "正确标注了可疑，判据却报违规（判据方向反了）")`
- **反向（照打可疑 ⇒ 必须被抓）**：`:294-302`，先断言未变异文本 `:295` 必须 `is_empty()`（对照，防恒为真），再断言把合法归属行替换进可疑输出后的 `mutant` 必须 `!is_empty()`

我独立构造两种输入验证判别方向（探针 PROBE5）：`HumanRevised` 与 `JSON null→Suspected` 两种文本喂 `verify_provenance_scope`，**都返回 0 条违规** ⇒ 「正确标注了形态可疑」时判据**放行**，方向正确。再用源码级变异 **M6** 坐实：把 `Suspected` 渲染成 `agent=（未声明）`（= 照打），`test_review_provenance_suspicious_shapes_are_labelled` **变红**。

⇒ **该失误确已修复，且修复方向正确、可被变异杀死。**

**(2) 那次作废的全量 —— 他自报的 637/0/1 是干净树上的吗？—— 是，但我不是靠日志信的，是自己重跑了一遍**

Dev 的三份日志自洽（我逐份重算）：

| 日志 | 时刻 | `test result:` 行数 | passed/failed/ignored |
|---|---|---|---|
| `baseline-true.log` | 11:57 | 45 | 619 / 0 / 1 |
| `after-full.log` | 12:08 | 46 | 635 / **2** / 1 |
| `final-full.log` | 12:17 | 46 | 637 / 0 / 1 |

`after-full.log` 里失败的两条正是 `test_review_provenance_suspicious_shapes_are_labelled` 与 `test_review_provenance_track_separation_survives` —— 变异期二进制留下的痕迹，与他自报的「那次作废」吻合。

**但这三份日志都只含测试输出、不含 cargo 的 `Compiling` 行，单凭它们无法证明终值那次跑在干净树上。** 所以我**在 `ca8cdc7` 上自己重跑了一轮完整全量**（§5），结果 **637 / 0 / 1**，与他的终值一致。**这才是干净树的证据。**

⇒ **该失误确实存在（他在 NOTES.md 第 7 条如实自报），处置正确。** 值得记的是：他的 `NOTES.md`「自造问题 1~7」把踩坑全列了出来（含 stash 往返丢工作、误删字段、乱码字符），**自报诚实度高于平均**，这一点在本轮验收中确实降低了核实成本。

---

### 2.5 全量 / 红线 / 产物 sha

**全量（我自己跑的，干净树）**

```
起跑 12:35:28，HEAD=ca8cdc7cad6c09445ea8fd9649650fa76ea19bc6，review_view.rs sha256=eff3c1af79ca8234…
命令：cargo test --no-fail-fast      结束 12:43:35，耗时 487s
口径：对 46 条 `^test result:` 行逐行取第 4/6/8 字段（passed/failed/ignored）求和
结果：637 passed / 0 failed / 1 ignored，46 条 test result: 行
```

| | 基线 `f90d537` | HEAD `ca8cdc7` | 差 |
|---|---|---|---|
| passed | 619（Dev 的 `baseline-true.log`，45 行） | **637**（我实测，46 行） | **+18** |
| failed | 0 | 0 | — |
| ignored | 1 | 1 | — |
| `test result:` 行数 | 45 | 46 | +1（新测试文件） |

+18 的构成我用测试函数名集合独立核对（不是相加得来的）：`diff` 基线 vs HEAD 的 `fn test_*` 名集合，**只有新增、零删除**：

- `tests/test_review_provenance.rs` **14** 条（新建文件）
- `tests/test_review_show_e2e.rs` **4** 条（既有文件内新增）

耗时 487s vs Dev 的 717s —— **口径相同但机器负载不同，我只声明我这一轮的数字，不声称稳定值**。

**红线**

| 红线 | 命令 | 结果 |
|---|---|---|
| 四文件零改动 | `git diff --numstat f90d537 ca8cdc7 -- src/domain/view.rs src/domain/review.rs tests/drop_rate.rs tests/test_fk_naming.rs` | **0 行输出** ✅ |
| `test_humanize_still_refuses_unknown_columns`（`src/domain/view.rs` inline `#[cfg(test)]`） | `git show f90d537:src/domain/view.rs` 与工作区文件各取 sha256 | **两者同为 `c0c2df2c…56e6`**，即**整文件逐字节一致** ⇒ 函数体必然一致（比 Dev 的「diff 一致」更强） ✅ |
| 无测试被删除 | `diff` 基线 vs HEAD 的 `fn test_*` 名集合 | **只增不删**（+18/-0） ✅ |
| 无 `#[ignore]` | `git grep -c "ignore"` 两版 | 基线 11 / HEAD 11，明细同为 `tests/test_mcp_server.rs:553` 那一条**既有** ignore ✅ |
| 测试文件集合 | `diff` 两版 `tests/` 文件列表 | 只多 `tests/test_review_provenance.rs` ✅ |

**产物 sha（独立复现）**

```
输入：/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json
      sha256 = b71c2afde6c103132e4e1ca6b9896aab442bd80abbe1fcc0870f35ddce0f2019
命令：cargo run --quiet --bin ontok-propose -- \
        --schema <上面那个绝对路径> \
        --out   .review-workspace/verify-req060/proposal.qa.json \
        --stats .review-workspace/verify-req060/stats.qa.json \
        --project extracted          # 未传 --names（遵 AC-005）
```

| 产物 | 我实测 sha256 | AC 期望值 | 结论 |
|---|---|---|---|
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | 同左 | ✅ 一致 |
| `proposal.json` | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | 同左 | ✅ 一致 |

另外用 `cmp` 与 Dev 存档逐字节比对：**两份产物均逐字节一致** ⇒ 本需求**确实没碰抽取产物**。

---

## 3. 变异测试表

统一口径：每个变异改 `src/domain/review_view.rs` 后跑
`cargo test --test test_review_provenance --test test_review_show_e2e --test test_review_view --no-fail-fast`
（三个二进制共 **44** 条：`test_review_provenance` **14** / `test_review_show_e2e` **11** / `test_review_view` **19**）。
红条数 = 三条 `test result:` 的 failed 求和。**编译失败一律不计为变异执行**（下表「编译错误数」列全为 0）。
每个变异跑完立即用 sha256 复原，复原后 `src/domain/review_view.rs` = `eff3c1af79ca8234ccf348fd17a22cd00a6c1ed775fc50de2b7f016ee1f49f64`，与变异前一致。

| # | 变异 | 编译错误 | 结果 | 运行条数 | 红了几条 | 杀死它的判据（测试名） |
|---|---|---|---|---|---|---|
| **M1** | **AC-004 真实陷阱**：把归属内容塞进 `ModelFacing.evidence`（中性文本，不含归属行前缀） | 0 | **杀** | 44 | **2** | `test_review_provenance_premise_holds_is_bit_identical_with_and_without_provenance`、`test_review_show_e2e_premise_holds_unaffected_by_provenance`（报 `hits=["item[1].evidence"]`、`premise_holds false→true`） |
| **M2** | 归属拼进**每个**条目头行 | 0 | **杀** | 44 | **5** | 判据③规则①（`归属行数为 3，应恰好 1 行`）+ 4 条用例 |
| **M2b** | **归属只拼进第 1 个条目头行**（` · ` 分隔开），删掉独立归属行 | 0 | **存活** ⚠️ | 44 | **0** | —— 无判据、无测试抓住 |
| **M2c** | M2b + 连「整份提案共用一份，不是逐条目的」措辞一并删掉 | 0 | **杀** | 44 | **1** | `test_review_provenance_wording_says_proposal_level` |
| **M3** | `Declared` 只打 `agent=`，不打 `version=` | 0 | **杀** | 44 | **2** | `test_review_provenance_declared_is_visible`、`test_review_show_e2e_provenance_survives_jsonb_roundtrip` |
| **M4** | JSON null 混进「人工修订」（`is_null()` 分支改落 `HumanRevised`） | 0 | **杀** | 44 | **1** | `test_review_provenance_suspicious_shapes_are_labelled` |
| **M5** | `NotProvided` 与 `HumanRevised` 合并（= 假安心） | 0 | **杀** | 44 | **1** | `test_review_provenance_not_provided_is_distinct_from_human_revised` |
| **M6** | 可疑形态**照打**成 `agent=（未声明）`（AC-003 的违规实现） | 0 | **杀** | 44 | **2** | `test_review_provenance_suspicious_shapes_are_labelled`、`test_review_provenance_track_separation_survives` |
| **M7** | 归属**逐条目重复打印**（AC-002 点名的误导变体，= Dev 变异 2） | 0 | **杀** | 44 | **5** | 判据③规则① + `per_item_repetition_is_caught` 等 4 条 |
| **M8** | `sourced_fields()` 无条件 `push("provenance")`（= Dev 变异 1） | 0 | **杀** | 44 | **8** | 含 `sourced_fields_has_no_provenance_entry`、`premise_holds_is_bit_identical…`、`e2e_premise_holds_unaffected…`、`test_review_view` 的 4 条既有 REQ-057 判据 |

**存活变异论证（仅 M2b，逐条）**

1. **触发条件**：渲染器把归属从「独立一行」改成「只拼在第 1 个条目的头行里，用 ` · ` 分隔」。
2. **观察到的行为**：44 条测试全绿；判据③报 0 条违规；判据②报 0 条违规；端到端那条「归属行数 == 1」也通过（确实只有 1 行）。
3. **为什么活着**：规则②只查 (i) 是否含两轨前缀、(ii) 行号是否晚于第一个条目行。`[N] … 状态=` 头行两条都不触。
4. **支持的影响**：**产品行为仍然正确** —— 该行仍带 `PROVENANCE_SCOPE_NOTE`，FDE 读到的仍是「整份提案的归属」；AC-002 不被违反。
5. **因此定性**：**判据③的文档承诺 > 实现**，属非阻断覆盖缺口（F-2），不是产品缺陷。
6. **反证已做**：M2c（在 M2b 基础上再删掉提案级措辞）**被杀** ⇒ AC-002 真正点名的那条误导路径有护栏。

---

## 4. 四个「是不是真的」

| 问题 | 我的判断 |
|---|---|
| **类型层手段真的成立吗？** | **成立，且是四路证据交叉的**：结构体字段位置（`review_view.rs:140` vs `:413-441`）、穷尽式结构体字面量的**构造证明**、接收者字段白名单（只读 6 个 `self.` 字段）、以及 `premise_holds`/`hits` 的逐 bit 实测。**但边界要说清**：它挡的是「给 `ModelFacing` **加字段**」，不挡「把已有文本**搬进** `ModelFacing.evidence`」—— 后者能让 `premise_holds` 变真，两条运行时判据都报 0 条，靠单元测试 M1 兜住。 |
| **四态真的分得开吗？** | **分得开**。四态输出两两不同；`JSON null` 与 `SQL NULL` 分得开（`Suspected` vs `HumanRevised`，且 M4 变异可杀）。**但 `NotProvided` 今天保护不到任何生产路径** —— 唯一生产入口 `cli.rs:636` 用的是三参版，`ReviewTrackView` 无任何持久化点。它是给未来的类型级护栏，不是现存需求的修补。 |
| **Dev 的两处失误真修了吗？** | **都真修了**。第 1 条（断言方向）现在是双向的，我用正向 + 反向双输入 + 变异 M6 独立确认方向正确；第 2 条（作废那次全量）他自报诚实、处置正确，但**日志本身证明不了干净树**，所以我自己重跑了一轮 —— **637/0/1，与他终值一致**。 |
| **他给 BA 的反馈你同意吗？** | **同意，而且我认为他这条比 AC 更值钱。** AC-001 把 `extracted_by` 为 `None` 当**一种**情况，实现里是**两种语义相反**的东西：SQL NULL（= 有人审过，可放心）与「调用方没提供」（= 我不知道，须警惕）。按字面实现会在第二种情况下给出**假安心** —— 那正是 REQ-056「猜错比不给更危险」的同构形态。他补的 `JSON null` 区分同理（`JSON null ≠ SQL NULL`，混进「人工修订」也是假安心），且有 M4 护栏。<br>**我加一条他没说的**：这个坑是**通用**的，不是归属特有的 ——「`None`」在 Rust 里同时承载「没值」「没接上」「形态不对」三种。建议后续 AC 把「`None` 其实是好几种东西」单列成一条常设检查项。 |

---

## 5. 发现但未修的问题

### F-1（低 · 夹具失真）测试夹具里的 `extracted_by` 形状与 REQ-059 的真实产物**不符**

- **证据**：我复现的产物里 `extracted_by.agent = "inproc:ontok::agent::RuleBasedAgent"`、`version = "0.1.0"`。
  而 `tests/test_review_provenance.rs:92-95` 的注释写着「**REQ-059 之后产物里真实出现的那份归属**」，取值却是 `{"agent":"mcode-agent","version":"2026.10.07"}`；`tests/test_review_show_e2e.rs:335-340` 同样。
- **影响**：`mcode-agent` 正是 REQ-059 **明确移除**的那个硬编码占位（REQ-059 后 `--agent` 变成断言，不匹配就拒绝出产物）⇒ 该取值**今天的 `ontok-propose` 根本产不出来**。机制本身对任意非空字符串都成立（我核对过逻辑，不受影响），但「JSONB 往返证明真实形状还在」这句话**名不副实**，且两处注释断言了一个关于 REQ-059 的**假事实**。
- **建议**：夹具改成 `inproc:ontok::agent::RuleBasedAgent` / `0.1.0`，或把注释改成「任意非空串的形状」。**非阻断。**

### F-2（低 · 判据覆盖缺口）`verify_provenance_scope` 管不到「归属只在某一个条目头行里」

见 §2.3(c) 与 §3 的 M2b。判据③ 文档（`:807-809`）声称能抓「塞进某一条的行」，实测抓不到 `[N] … 状态=` 头行这种形态。**AC-002 本身仍被守住（M2c 被杀）**，所以是覆盖缺口而非缺陷。**非阻断，建议 BA 定夺是否开后续小条。**

### F-3（极低 · 渲染措辞）`NotProvided` 行里用了 Markdown 星号

`render_provenance_line`（`review_view.rs:936`）产出 `**不是**「人工修订」`。`review show` 是**纯文本终端输出**，星号会原样显示成 `**不是**`。不影响任何断言（F-1 之外的纯观感项）。**非阻断。**

### Dev 已自报、确认不在本条 scope 的两项（我同意归为遗留债）

- `ProposalProvenance.failed_groups` 是 `Vec<Value>` 而快照侧是 `Vec<String>`（REQ-022 遗留）—— 当前投影侧用 `render_group_ref` 做了透传兜底，**不会静默丢数据**，可接受。
- `source` 两侧含义不同（我核实：产物里 `source = "introspect … read_only=on"`，是勘查来源，不是 agent 来源），接进来会重演 ADR-003 误读。**同意暂不接。**

---

## 6. 我自己引入又清掉的问题（如实报）

1. **探针文件放错位置**：第一版 `tests/test_verify_req060_probe.rs` 把 `project_review_track_with_summary_and_provenance` 的返回值当成 `(String, ProposalStatus)` ⇒ 18 个 `E0308/E0599` 编译错误。**已改**（返回值是 `(ReviewTrackView, ProposalStatus)`，需另调 `render_review_text`）。
2. **探针自己的断言写错**：`probe5` 断言「JSON null 行不含『人工修订』」⇒ 失败。**是我的断言过严**，不是产品缺陷 —— 该行确实含这四个字，但在「**不是**人工修订的 SQL NULL」的否定语境里。改为精确断言 `不含「人工修订，ADR-001」` 后通过。
3. **探针 8 的 format 参数个数错**：写 `\"{}{}{}{}\"` 传 3 个参数 ⇒ 编译错误。已修为 3 个。**（编译失败不计变异执行 —— 这两处都是探针自身编译失败，未混入任何变异结果。）**
4. **临时文件的处置**：探针文件在变异阶段**已移出 `tests/`**（移到 `.review-workspace/verify-req060/test_verify_req060_probe.rs.bak`），避免给变异日志添噪声。**未留在 worktree 里**。
5. **8 次源码变异全部按 sha256 复原**，复原后 `review_view.rs` = `eff3c1af…9f64`，与变异前一致；最终 `git status -uall` **为空**，HEAD 仍为 `ca8cdc7`，未改动任何历史、未 push。
6. **本机 `psql` / `psycopg` 确认不可用** ⇒ 按 Dev 同样口径，**未测 `SELECT 1` p50**（不声称该指标）。

---

## 7. 附：命令与日志索引（全部在 `.review-workspace/verify-req060/`）

| 文件 | 内容 |
|---|---|
| `t1-provenance.log` | Dev 的 14 条纯逻辑测试（含四态打印原文） |
| `t2-probe.log` | **我的 8 条独立探针**（PROBE1~8 全文输出） |
| `mut-M1.log` ~ `mut-M8.log` | 10 次变异运行日志 |
| `verify-full.log` | **我的干净树全量**（637/0/1，46 行） |
| `propose-run.log` | 产物复现运行输出 |
| `proposal.qa.json` / `stats.qa.json` | 我复现的产物（与 Dev 存档 `cmp` 逐字节一致） |
| `review_view.rs.pristine` | 变异前源文件备份（sha256 `eff3c1af…`） |
| `view.baseline.rs` | `git show f90d537:src/domain/view.rs`（与工作区同 sha256） |
| `mutate.py` | 变异注入脚本（每次先从 pristine 复原再改） |
| `PARTIAL.md` | 中途落盘的草稿（会话丢失防护） |
