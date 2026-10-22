# REQ-116 · 文档摄取与断言抽取 —— 验证报告（**第 2 轮 · QA 6 条非阻断：N-2 / N-5 改判阻断已修**）

> **被测**：worktree `feature-REQ-116` / 分支 `feature/REQ-116` · 基线 `develop` `a85383a` · **未 push**
> **本轮范围**：QA 判 PASS 带 6 条非阻断，BA 把 **N-2 / N-5 改判阻断**。本轮修 N-2 / N-5，补齐 N-1 / N-4 / N-6；N-3 只登记不修。
> **不碰**：`BA/` / `code/` / `Deploy/`；四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4；`/api` 鉴权与路由；`Cargo.*`（零新依赖）。
> **日期**：2026-10-22 · Windows · PG `postgresql://***@172.17.0.87:5432/anspire_real`（**只读**指纹）

---

## ⚠️ 三条必须先读的结论

1. **N-2 是真 bug，不是文档问题**：`belongs_to` 在**计数之前**就被谓词闸 `continue` 掉，
   所以「活下来的谓词分布」里永远没有它 ⇒ AC-004 的「< 20%」**结构性恒真**。
   修法不是改判据，而是**先记原始分布、再过闸**（`raw_predicate_counts`），
   并让 `belongs_to` 占比按**原始输出**算（`belongs_to_share.raw`）。
2. **N-5 也是真 bug**：`walk_docs` 里 `if name.starts_with('.') { continue; }`，
   注释却写「跳过并在 stderr 说明」——实际一声不吭，隐藏目录里的 `.pdf` 让整条命令 **exit 0**。
   本轮按 **(a)** 修：隐藏项**照常处理、照常报错**（`.md` 照常读）。
3. **N-4 的漏洞在判据层**：QA 把 `locate_quote` 改成前缀匹配时 lib 红 4、集成 8 条全绿。
   本轮给假端点加了一种回答：**原文前缀 + 原文没有的尾巴** —— 逐字匹配必拒，
   任何「前缀/半句/starts_with」放宽都会放行它。变异实测：集成层**立刻红**。

---

## 0 · 结论先行（对应 QA 的 6 条）

| QA 项 | 交付 | 实测 |
| --- | --- | --- |
| **N-2** 两个新计数 + 原始占比 | ✅ `raw_predicate_counts` / `rejected_by_predicate_gate{total,by_predicate}` / `belongs_to_share{raw,alive}`；屏幕同时打两个占比 | ✅ 构造含 `belongs_to` 的输出：**raw 14/28 = 50.0%**（alive 0/14 = 0.0%）；真语料 raw 0/51（真 0，分母非 0） |
| **N-5** 选 (a) 还是 (b) + 实测 | ✅ 选 **(a) 照常处理**；注释与行为一致 | ✅ 隐藏目录 `.pdf` ⇒ exit 1 + 点名 + 不产文件；隐藏 `.md` ⇒ exit 0 + 1 块 |
| **N-4** 新集成用例 + 变异必须红 | ✅ `ac003_prefix_plus_invented_tail_is_dropped` | ✅ 变异后：lib 3 红 · **集成该用例红**（修前同变异下集成全绿） |
| **N-6** 文案 | ✅ 报错用实际后缀 | ✅ `.PDF` 显示 `.PDF`、`.pdf` 显示 `.pdf` |
| **N-1** 三份文档 | ✅ `status.md` / `verification-report.md` / `manifest.json` | ✅ 数字全部来自本轮实测（见 §7） |
| **N-3** 只登记 | ✅ 写进已知限制 | ✅ 「语料过拟合，只能当冒烟夹具；AC-004 等业务文档」 |

### AC 对照（本轮复核）

| AC | 判据 | 实测 |
| --- | --- | --- |
| 001 | 只吃 `.md/.txt`；`.pdf/.docx/.doc` exit≠0 点名 + 提示转文本；⛔ 不静默跳过 | ✅ 三种格式各一例 + **`.PDF` 原样后缀** + **隐藏项**（N-5）；格式被拒时不出产物 |
| 002 | 同参数两次分块逐字节相同 | ✅ `--chunks-only` 两次 sha256 `8DAD2AC0…5CEA2` 相同（14 块） |
| 003 | `quote` 必须在该块原文里逐字找到，否则丢弃 + 计数；⛔ 不修正 | ✅ 真语料 101 条 span **独立复核 `bad=0`**；伪造 / **前缀+尾巴** 两种 e2e 全丢弃 |
| 004 | 谓词是文档里的动词；`belongs_to` 占比 < 20% | ✅ **原始 0/51 = 0.0%**（可测）；结构词被硬拒且**计入原始分布** |
| 005 | `modality ∈ {fact,spec,future,example}`；future/example 不可对齐 | ✅ fact 86 · spec 11 · future 3 · example 1；4 条 `alignable=false` |
| 006 | 第二次运行 **0 token**、JSONL 逐字节相同 | ✅ 0 token / 14 命中 / 同 sha256 `A55FE9CD…99056` |
| 007 | 越预算 ⇒ 停 + 报「已用/上限/停在第几块」+ 不产半份 JSONL | ✅ `--budget-tokens 1` ⇒ exit 1、`已用 636 / 上限 1 / 停在第 2 块`、无断言文件、缓存落盘 |
| 008 | 不写库 | ✅ 7 张表（含 `ai_cache`）内容 sha256 前后一致；模块内无 `sqlx`/`PgPool`/`db::` |
| 009 | 产物登记进 `OUTLETS`（内部轨，不过客户闸） | ✅ `I10`（`InternalOnly` / `Track::Internal`），客户出口零引用 |
| 010 | 零新依赖 / 不退化 | ✅ `Cargo.*` 无 diff；四红线无 diff；无删测试 / 无 `#[ignore]` |
| 011 | 限制如实登记 | ✅ 见 §8（含 N-3 语料过拟合） |

---

## 1 · N-2：把「恒真判据」改成「可测判据」

### 1.1 病在哪

```rust
// 旧：谓词闸在计数之前
if predicate.is_empty() || is_structural_predicate(&predicate) {
    stats.reject(Rejection::Predicate, ...);
    continue;                        // ← 这里之后才会 predicate_counts += 1
}
...
*stats.predicate_counts.entry(predicate.clone()).or_insert(0) += 1;
```

`belongs_to` 永远到不了第二行 ⇒ `predicate_counts` 里没有它 ⇒ 占比 **0/49 = 0%** 恒成立。

### 1.2 改法

| 落点 | 改了什么 |
| --- | --- |
| `src/extract/doc_ingest.rs:998` | **先记原始分布**（`raw_predicate_counts`，空谓词记 `(空谓词)`），**再过闸** |
| `:1290-1294` | `DocStats` 新增 `raw_predicate_counts` / `rejected_by_predicate_gate` / `rejected_predicate_counts` |
| `:1301-1319` | `PredicateShare { count, total, share: Option<f64> }` —— 分母为 0 时 `null`，**不冒充 0%** |
| `:1336-1356` | `predicate_share()`（活下来）与 `raw_predicate_share()`（原始）**两个函数**，分母分别是 `relations` 与 `raw_relations()` |
| `:1570-1611` | `stats.json` 新增 `raw_relations` / `raw_predicate_counts` / `rejected_by_predicate_gate` / `belongs_to_share` |
| `src/cli.rs:1351-1375` | 屏幕同时打：`谓词（原始）`、`belongs_to : 原始 … ← AC-004 判据 · 活下来 …`、`谓词闸丢弃 N 条（分布）` |

### 1.3 实测

**A · 构造含 `belongs_to` 的模型输出**（假端点，14 块，每块 2 条关系：`下单` + `belongs_to`）：

```json
"raw_relations": 28,
"raw_predicate_counts": {"belongs_to": 14, "下单": 14},
"rejected_by_predicate_gate": {"total": 14, "by_predicate": {"belongs_to": 14}},
"belongs_to_share": {
  "alive": {"count": 0, "total": 14, "share": 0.0},
  "raw":   {"count": 14, "total": 28, "share": 0.5}
}
```

⇒ **原始占比 50%（非 0）**，活下来 0%。这正是 N-2 要的那条：判据不再恒真。

**B · 真语料（qwen36-27B，14 块）**：

```
谓词（原始）: … （51 条关系，含被谓词闸丢掉的 0 条）
belongs_to  : 原始 0/51=0.0% ← **AC-004 判据** · 活下来 0/49=0.0%
```

⇒ 真语料里 `belongs_to` **确实是 0**，但这是**测出来的 0**：原始分母是 51（比活下来的 49 多 2，
那 2 条是「保存 / 说明」因**证据**被丢，不是谓词闸）。**分母非 0** 才说明判据有判别力。

单测：`raw_belongs_to_share_is_not_structurally_zero`（50% vs 0%）、
`structural_predicates_are_rejected_but_counted_raw`（原始有、活下来没有）。

---

## 2 · N-5：隐藏项不再静默跳过（裁定 **(a)**）

**选 (a) 的理由**：(b)「跳过但逐个点名 + 计数」会引入两套口径（哪些隐藏项算文档？谁决定？），
而 AC-001 的原话是「跳过等于装作处理过」。最省歧义的规则是**没有例外**：
`.md`/`.txt` 照常读，其它格式照常报错 —— 隐藏与否不参与判断。

```rust
// 旧（fail-open）
let name = p.file_name()...;
if name.starts_with('.') { continue; }   // ← 注释说「会在 stderr 说明」，其实没有

// 新：没有第三条路
if p.is_dir() { walk_docs(root, &p, acc)?; continue; }
check_ext(&p)?;                          // 隐藏项也走同一道格式闸
```

**实测**：

```text
# 隐藏目录里的 .pdf
✗ 不支持的文件格式：.pdf（out\req116\demo-hidden\in\.hidden\x.pdf）
… ⛔ 不会静默跳过 —— 跳过等于装作处理过（AC-001）。
exit=1  assertions存在=False

# 隐藏的 .md
✓ 只分块（--chunks-only）：1 个文件 → 1 块
file=.secret.md chunks=fec141d13b87:0-25
```

**代价（已登记）**：别把仓库根 / 家目录当 `--input` —— `.git` 里的非文档文件会按 AC-001 报错。

---

## 3 · N-4：集成层对「放宽为前缀匹配」有牙（M2 变异实测）

新增假端点回答方式（`tests/test_req116_doc_ingest.rs:573`）：

```text
quote = 该块原文的前 40 个字 + "（原文没有的后半句）"
逐字匹配 ⇒ 拒；前缀 / 半句 / starts_with ⇒ 放行（用例立刻红）
```

**变异**（临时把 `locate_quote` 改成「quote 的前一半能在原文里找到就放行」）：

```text
=== 变异后：lib ===
failures:
    extract::doc_ingest::tests::forged_quote_is_rejected
    extract::doc_ingest::tests::forged_quotes_are_dropped_and_counted
    extract::doc_ingest::tests::prefix_only_match_is_not_accepted
test result: FAILED. 21 passed; 3 failed

=== 变异后：集成 ===
test ac003_prefix_plus_invented_tail_is_dropped ... FAILED
test result: FAILED. 9 passed; 1 failed
```

**还原**：`RESTORE doc_ingest: before=87A7E9BC… after=87A7E9BC… match=True`。

> ⚠️ 一个必须记下来的坑：`Copy-Item` 还原会**保留旧 mtime**，cargo 因此认为「无需重编」，
> 我一度跑的是**变异版产物**（一批假红）。现在还原后一律 `touch`（或 `cargo clean -p ontok`）再跑；
> 报告里的数字全部来自**重编后**的那一次。

---

## 4 · N-6：报错显示**实际后缀**

```text
# 旧：extension_of() 先 to_ascii_lowercase()
✗ 不支持的文件格式：.pdf（…\sample.PDF）      ← 用户写的是 .PDF

# 新：raw_extension() 报错 / lower_extension() 判能不能吃
✗ 不支持的文件格式：.PDF（out\req116\demo-upper\SAMPLE.PDF）
exit=1
```

判据：`unsupported_formats_are_named` 里加 `.PDF` 用例，断言「必须含 `.PDF`」「不许含 `.pdf（`」。

---

## 5 · 真语料实测（第 1 轮的数字，本轮重放仍逐字节一致）

| 项 | 值 |
| --- | --- |
| 文件 / 块 | 4 / 14（1 份 `.txt` + 3 份 `.md`） |
| 断言 | **101**（概念 52 · 关系 49）；丢弃 2（都是模型把 quote 写成带「…」的省略句 ⇒ 证据校验抓住） |
| span 独立复核 | **checked=101 bad=0**（Python 按 UTF-8 字节切片，**不复用**被测 `locate_quote`） |
| 谓词 | 活下来 49 条 / 37 个不同谓词；**原始 51 条**；`belongs_to` 原始 **0/51 = 0.0%** |
| 模态 | fact 86 · spec 11 · future 3 · example 1（future/example 共 4 条 `alignable=false`） |
| token | 首跑 13643（含 1 次端点 502，失败**不缓存**）→ 补跑 1804 → 第三次 **0** |
| 缓存 | 14/14 命中；JSONL sha256 `A55FE9CD…99056` 两次一致 |

---

## 6 · 格式闸 / 预算闸 / 不写库（回归复核）

```text
# .pdf
✗ 不支持的文件格式：.pdf（…sample.pdf）
本命令只吃 .md / .txt（UTF-8 纯文本）。
请先把它转成 .md/.txt 再跑（转换后请记下 hash）：pandoc …
⛔ 不会静默跳过 —— 跳过等于装作处理过（AC-001）。
exit=1

# --budget-tokens 1（全新缓存）
✗ 预算超限：已用 636 / 上限 1 token，停在第 2 块（共 14 块）
exit=1 · assertions=False · stats=False · cache=True

# 跑前 / 跑后（只读会话）——7 张表内容 sha256 全部相同
object_types 2151 / link_types 836 / proposals 764 / proposal_items 2507
ontology_versions 1901 / concept_groups 403 / ai_cache 82（⛔ 没碰 REQ-109 的表）
```

---

## 7 · 测试数字 + 绕行双证（**本轮实测**）

```text
lib       : 315 passed / 0 failed        （基线 291 + 本需求 24 条单测）
doc-tests : 4 passed
bin 单测  : 3 个目标 / 14 passed
tests/*.rs: 77 个目标 / 813 passed / 8 failed（5 个用例）
合计      : 1146 passed / 8 failed
新增套件  : tests/test_req116_doc_ingest.rs —— 10 passed（含 N-4 / N-5 两条新判据）
```

**8 个失败全部既有 / 环境，与本需求无关**（5 个用例）：

| 用例 | 原因 | 证据 |
| --- | --- | --- |
| `drop_rate` | 本机没有 `.review-workspace/fix-props/schema.json`（fail-closed 门禁，缺输入必红） | 报错原文「读不到脱敏 schema.json」 |
| `test_fk_naming` | 同上 | 同上 |
| `test_agent_orchestration::test_cli_end_to_end_…` | REQ-108 的置信度路由把「无数据证据」的关系挪进 `proposal-pending.json`，而这条 REQ-015 老用例拿 `proposal.json` 的 items 逐字节比 | 该用例目录里 `proposal-pending.json` 恰好只有 `add_link_type 设备→客户` |
| `test_req084_…at_its_line` / `…no_unregistered…` | 登记表在 **develop 上就已过期** | `git show develop:src/extract/propose.rs` 第 280 行是 `/// 真正的 agent 抽象…`，登记表说那里是 `customer_rebate`；`git diff develop -- src/extract/propose.rs src/domain/concept.rs tests/retained_names.req084.tsv` 为空 |
| `test_provenance_source`（3 条） | 共享库里**项目名撞名**（该测试自己在 `tests/common/mod.rs:457` 说明「pid 会被回收复用 ⇒ 会真的撞名」） | 单跑 **5 passed / 0 failed**；本轮全量刚好撞上 |

> 📌 与 BA 给的口径对账：BA 说 QA 实测「813/4」。本轮 **813 passed / 4 个目标**（drop_rate + fk_naming +
> agent_orchestration + retained），第 5 个目标 `test_provenance_source` 是本轮新撞上的（单跑绿）。
> 上一轮我报的「802/75」确实是 **REQ-115 的数字**（N-1 的批评成立）。

**绕行双证**（Windows 上 `src/bin/ontok-gate-preflight.rs` 是 unix-only，且被
`tests/test_retained_names_req084.rs` 用 `include_str!` 编进来）：

1. `src/bin/ontok-gate-preflight.rs` 挪成 `.moved`（cargo 只编 `*.rs`）；
2. retained 用例里**只改那一行** `include_str!` 指向 `.moved`（它按逻辑路径查 content，内容没变 ⇒ 判据不变）；
3. `finally` **逐字节还原**：

```text
RESTORE preflight: before=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
                   after =C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D match=True exists=True
RESTORE retained : before=2DB7D22C67DAB4C628DB927D5CC65EBB37CC15A72928196C5F83731980904CF9
                   after =2DB7D22C67DAB4C628DB927D5CC65EBB37CC15A72928196C5F83731980904CF9 match=True exists=True
GIT-STATUS: （只有本轮改动的三个文件 + 无 D 行）
```

---

## 8 · 如实登记（AC-011）

1. **N-3 语料过拟合（QA 判定，接受）**：4 份自造语料是**看过提示词之后**写的 ——
   `04` 的「术语对照」恰好是规则 2 的示例、`03` 逐字复刻规则 4 的「计划/将/待定/尚未」、
   `01` 首段先把要抽的概念列出来。⇒ **只能当冒烟夹具**；AC-004 的真实分布**等业务文档**。
   本轮**没有**为此改语料（改了就是掩盖），只把这条写进模块头与三份文档。
2. **缓存是本地文件**（`<out>.cache.json`），不是 `ai_cache` 表：为了让「不写库」是**结构性事实**。
   与 REQ-109 同的是**指纹口径**，不同是存储介质。若要复用表：`DocCache` 是注入式 trait，改动面只在存储层。
3. **`.txt` 没有标题层级**（只按段落 + 1200 字符上限分组，绝不切句）。
4. **隐藏项照常处理** ⇒ 别把仓库根 / 家目录当 `--input`。
5. **`--budget-tokens` 省略时默认 500000**（stderr 明说）。
6. **`stats.truncated` 只在 `--limit-chunks` 时可能为 true**；越预算那条路径是 `bail!` + 不写产物，
   所以文件里的 `budget_stopped_at` 实际恒为 `null`（字段在，走不到）。
7. **谓词质量**：49 条里有「有 / 针对 / 说明 / 走」这类弱谓词（模型从原文取的字）。
   AC-004 的可测判据（`belongs_to` 原始占比 < 20%）是 **0%**，但「像不像动词」需要真实业务文档再调提示词。
8. **粒度未调优**：14 块 / 101 条是冒烟规模；真实文档的块大小与断言密度未验证。

---

## 附 · 本轮改动（path:line）

| 文件 | 落点 | 改了什么 |
| --- | --- | --- |
| `src/extract/doc_ingest.rs` | `:80` | 模块头：隐藏项照常处理 + N-3 过拟合登记 |
| 同上 | `:217-236` | `unsupported_format_error` 文档：`ext` 是**原样后缀**（N-6） |
| 同上 | `:294-309` | `raw_extension` / `lower_extension` 拆开（判据 vs 文案） |
| 同上 | `:365-424` | `walk_docs` 不再跳过隐藏项；`check_ext` 用原样后缀报错（N-5 / N-6） |
| 同上 | `:998-1030` | **N-2**：先记 `raw_predicate_counts`，再过谓词闸并记 `rejected_by_predicate_gate` / `rejected_predicate_counts` |
| 同上 | `:1282-1319` | `DocStats` 三个新字段 + `PredicateShare` + `EMPTY_PREDICATE_KEY` |
| 同上 | `:1336-1356` | `predicate_share` / `raw_predicate_share` / `raw_relations` |
| 同上 | `:1570-1650` | `GateStats` / `BelongsToShare`；`StatsReport` 新字段 + `render_stats` 填充 |
| 同上 | `:1924` / `:1952` | 单测：结构词被拒**但计入原始**；原始占比 50% vs 活下来 0% |
| 同上 | `:2162` / `:2181` | 单测：`.PDF` 原样后缀；stats.json 的 N-2 四个字段 |
| `src/cli.rs` | `:1289-1375` | 两个占比 + 原始分布 + 谓词闸丢弃进屏幕报告 |
| `tests/test_req116_doc_ingest.rs` | `:311/332` | N-6 `.PDF` 用例 + **N-5** 隐藏项两条用例 |
| 同上 | `:494-525` | N-2：原始分布 / 谓词闸 / 两个占比的断言（`raw_share > 0`） |
| 同上 | `:573` | **N-4**：前缀 + 原文没有的尾巴 ⇒ 必须丢弃（`PrefixForged` 模式） |
| `.feature/manifest.json` / `status.md` / `verification-report.md` | 全篇 | **N-1**：写成本需求，数字用本轮实测 |

---

# 历史：REQ-115 · 概念分组与图上的归并 —— 验证报告（第 2 轮 · BA 裁定 C 修复）

> **被测**：worktree `feature-REQ-115` / 分支 `feature/REQ-115` · 基线 `develop` `71ae770` · **未 push**
> **本轮范围**：QA 判 FAIL（1 阻断）后的修复。BA 纠正了「`scope_hint` 是评审注解」这个前提 ——
> 它在 REQ-010 里就是**客户可见的组说明**（业务总览的 `summary`），病灶是**一个字段两种用途**。
> 按裁定 (C)：**C-1** 组字段纳入导入闸、**C-2** `suggest` 不写客户字段、**C-3** 图与总览同口径。
> **不碰**：`BA/` / `code/` / `Deploy/`；四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4；
> `/api` 的鉴权与路由语义。
> **日期**：2026-10-22 · Windows · PG `postgresql://***@172.17.0.87:5432/anspire_real`
> **历史报告**：第 1 轮（实现）与 B 轮（被 C 取代）见 `git log -p -- .feature/verification-report.md`。

---

## ⚠️ 三条必须先读的结论

1. **QA 阻断的根因是「一个字段两种用途」**，不是某个出口漏了：
   `scope_hint` 在 REQ-010 的**业务总览**里是客户可见的 `summary`（C4/C5/M1），
   却又被 `concept suggest` 当成内部备注写（「算法建议…待人审」）。
   ⇒ 本轮按 (C) 把**客户可见的组字段全部纳入导入闸**（C-1），并让建议层**不再写客户字段**（C-2）。
2. **AI 那份声明现在会被 import 拒绝一次**（这是**正确行为**）：
   `api_entitlement.scope_hint` 里的「未持 **Key**」命中闸 ⇒ 报
   「组 `api_entitlement` 的 scope_hint 含建模术语 `key`」。
   把该句改成「未持**凭证**」、重算 digest ⇒ **全链路通过**（200 / 8 组 / 83 成员）。
   **我们不替作者猜业务措辞**：报错点名到「组 + 字段 + 词条」，改不改、怎么改由声明作者定。
3. **C-3 我选了「图带 `scope_hint`」**（与总览同口径）。因此 QA 第 1 轮的 B-1（从图上删掉）
   **被回滚**；安全边界改由 C-1 在**导入时**兜住 —— 出图时不可能再遇到脏字段。

---

## 0 · 结论先行（对应 BA 的 6 项交付）

| BA 交付项 | 实测 |
| --- | --- |
| ① C-1 实现位置 + 三种拒绝实测 | ✅ `concept.rs:349` / `cli.rs:1211`；三种见 §2（scope_hint 拒 / display_name 拒 / 两者干净通过） |
| ② C-2 改完 `suggest` 的 `scope_hint` 长什么样 | ✅ 9 个组**全部是 `"scope_hint": null`**（§3）；来源写在 `declared_by` / `rule` |
| ③ C-3 选哪个口径 + 总览是否恢复 200 | ✅ 选「图带、总览也带」（同口径）；`ontology overview` 在 v4 上 **exit 0**（§4） |
| ④ AI 原样 import 被拒（报组/字段/词条）→ 改一句 → 全链路 200 + 8 组 | ✅ 见 §2.4 / §4.2 |
| ⑤ `ontology overview`（新发现的 C4/C5 出口） | ✅ C4 `exit 0`、C5 `--json` `exit 0`（§4.3） |
| ⑥ 测试数字 + 绕行双证 | ✅ 见 §7 |

| AC | 判据 | 实测 |
| --- | --- | --- |
| 001 | `/api/graph` 返回 `groups`，节点 `group` 等于某组 `id` 或 `null` | ✅ 8 组 / 83 节点 / `bad=0`；无组项目 `groups:[]` + 83 个 `null` |
| 002 | 组圈 + 折叠（组名 + N）+ 组面板（成员 / 点击定位） | ✅ 源码落点 + Node 探针 7 = 3 + 4 |
| 003 | 无组时如实显示「未分组」，⛔ 无默认组 | ✅ 探针：`groups=[]` ⇒ 零聚合单元 |
| 004 | `concept suggest` 两次逐字节相同 + 不写库 | ✅ C-2 后两次同 sha256 `abc9068a…`；库指纹前后相同 |
| 005 | suggest → import → 签 → apply → 图上出现 | ✅ 9 组与 AI 8 组（改一句后）各跑一遍（§4.2） |
| 006 | 页面不许自己算分组 | ✅ 只读 `DATA.groups`；剥注释后 7 个算法词全 0 |
| 007 | 零外部依赖 / `GET /` 零数据壳 / `/api/*` 401 | ✅ 页面 49936 字节 < 80KB；无 token → 401 |
| 008 | H1 重锚；其余 12 份不动 | ✅ `pipeline_bytes` 1 + `zero_visible_change` 3 全绿 |
| 009 | 折叠前后导出逐字节相同 | ✅ TTL / `ontology show` sha256 两两相同 |
| 010 | `source: "algorithm-suggest"` / `reviewed_by: null` | ✅ 文件字段 + 单测 |
| 011 | 不退化 | ✅ 见 §7 |

---

## 1 · 本轮改动（C-1 / C-2 / C-3）

| 文件 | 落点 | 改了什么 |
| --- | --- | --- |
| `src/domain/concept.rs:321` | `pub struct GroupNameHit` | 「组 + 字段 + 词条」的结构化命中；`Display` 就是报错原文 |
| `src/domain/concept.rs:349` | `pub fn scan_group_names` | **C-1**：扫 `display_name` / `scope_hint` / 组 `key` 三个**客户可见**字段 |
| `src/domain/concept.rs:366` | `fn push_hits` | 判据**借用管线**（`pipeline::group_name_hits`），不新增第二份判据 |
| `src/domain/pipeline.rs:893` | `pub fn group_name_hits` | 术语判据的唯一实现（`scan_modeling_terms`）在管线／守门器侧的入口 |
| `src/cli.rs:1211` | `concept import` | 校验指纹之后、碰库之前：命中即 `bail!`，逐条列出「组 + 字段 + 词条」 |
| `src/domain/concept_suggest.rs:201` | `suggest` 的组构造 | **C-2**：`scope_hint: None`（内部来源在 `declared_by` / `rule`） |
| `src/domain/graph.rs:125` | `Group` | **C-3**：恢复 `scope_hint`（与总览 `summary` 同源、同口径） |
| `src/domain/graph.rs:157` | `DeclaredGroup` | 同步恢复；`declared_groups` / `project_grouped` 照搬 |
| `src/server/page.rs:1160` / `:1174` | `showGroups` / `showGroup` | 组面板照常显示**客户可见**的组说明（C-3） |
| `tests/test_req115_groups.rs` | 13 条 | C-1 三种实测（scope_hint 拒 / display_name 拒 / 干净通过）+ 页面同口径断言 |
| `src/domain/concept.rs` / `concept_suggest.rs` 测试 | +4 条 | scope_hint 拒 / 业务措辞通过 / 组 key 也判 / suggest 不写客户字段 |
| H1 golden | `d8bd4581…`(32396) → **`c8d5e148…`(49936)** | 页面第二次变化（组说明回到面板），其余 12 份逐字节未动 |

> 第 1 轮实现（`/api/graph` 的 `groups`、页面组圈/折叠/面板/引力、`concept suggest`）见 `git log`；
> 本轮**没有**动 `overview.rs`、没有动任何 golden 的内容（只重锚 H1 的页面字节）。

---

## 2 · C-1：导入闸的三种实测（真库）

判据在 `concept import` 的**碰库之前**（`decl.verify()` 之后、`get_or_create` / `proposals.create` 之前）
⇒ 被拒的声明**一条提案都不会留下**。

### 2.1 ① `scope_hint` 命中（AI 声明**原样**）

```text
ontok concept import --file code/out/concept-declaration-ai.json \
                     --project anspire_excl_r115 --base-version 1
exit 1
✗ 概念声明被拒绝导入：有 1 处组字段会撞上客户发布闸（这些字段会出现在 /api/graph 的 groups[] 与业务总览的 summary 里）——
  - 组 `api_entitlement` 的 scope_hint 含建模术语 `key`
请改措辞后重算指纹（ontok concept digest --file …）再导入。⛔ 不会静默剔除，也不会替你改写。
```

### 2.2 ② `display_name` 命中（干净 `scope_hint` + 坏组名）

```text
（把 customer_account.display_name 改成「客户与 Key 子账号」，重算 digest）
exit 1
✗ 概念声明被拒绝导入：有 1 处组字段会撞上客户发布闸（…）——
  - 组 `customer_account` 的 display_name 含建模术语 `key`
```

### 2.3 ③ 两者都干净 ⇒ **导入成功**

```text
（把 api_entitlement.scope_hint 的「未持 Key」改成「未持凭证」，重算 digest）
概念声明 v1.0.0 已导入提案 p1（项目 anspire_excl_r115 · 基于 v1）—— 共 8 个概念组，待双轨审核
  [1] add_concept_group 客户与子账号 … [8] add_concept_group 后台与运营支撑
```

> 字段名**点名到列**（`display_name` / `scope_hint` / `key`），因为三者都可能成为
> 客户出口里的文本：图上 `groups[]` 三个都在，总览 `summary` 来自 `scope_hint`。

---

## 3 · C-2：`suggest` 不写客户字段

`concept suggest` 的组现在是：

```json
{
  "key": "concept_01",
  "display_name": "后台管理员",
  "members": ["admin_user_roles", "admin_users"],
  "scope_hint": null,
  "low_confidence": true,
  "extracted_by": null,
  "evidence": []
}
```

真库实测（`anspire_excl` v2）：

```text
groups = 9
groups with scope_hint = 0          （9 个组全部 null）
sha256(a) = abc9068ac018b93add267d17543489a753c8c72b2a0175e8cf31723f9654e953
sha256(b) = abc9068ac018b93add267d17543489a753c8c72b2a0175e8cf31723f9654e953  （两次相同）
```

内部来源仍在 `declared_by = concept-suggest/greedy-modularity-v1` 与 `rule` 里
（它们**不进客户视图**）；客户字段不再出现「算法建议…」这种内部话。

---

## 4 · C-3 口径 + 两个出口的实测

### 4.1 选择

**图带 `scope_hint`，总览也带** —— 两个出口**同一口径**。理由：
它在产品原有语义里就是客户可见的组说明（总览 `summary`）；删除它等于改 REQ-010 的产品语义，
而那不是本需求的授权范围。安全性由 C-1 在**导入时**保证。

### 4.2 AI 声明：原样被拒 → 改一句 → 全链路 200

```text
review decide --proposal p1 --accept-all --project anspire_excl_r115
  提案 p1 的 8 个条目 × 2 轨 → accepted
proposal apply → 提案已应用 → v4
version freeze --version 4 → 已冻结 v4
```

```text
GET /api/graph?project=anspire_excl_r115&version=4   (Bearer)
  status = 200 · nodes=83 · edges=52 · groups=8 · covered=83 · bad node.group = 0
  raw contains literal "key" = False
  raw contains scope_hint    = True   ← C-3（与总览同口径）
```

```json
"groups":[
 {"display_name":"客户与子账号","id":"customer_account",
  "members":["客户","客户账号绑定","账号渠道归属","客户功能权限","子账号权限","绑定权限快照","子账号额度"],
  "scope_hint":"客户主档、账号绑定关系、客户内部员工与子账号权限、客户功能开关。…"},
 {"display_name":"API 密钥与授权","id":"api_entitlement","members":["授权", …],
  "scope_hint":"客户手上有什么凭证、能调到什么、能调多快：API-Key（…）、以及未持凭证的试用请求。…"},
 …
]
"nodes":[{"attribute_count":10,"group":"marketing_growth","id":"活动","label":"活动","parent":null},
         {"attribute_count":7,"group":"admin_ops","id":"后台角色","label":"后台角色","parent":null}]
```

### 4.3 新发现的出口（C4/C5）已验掉

```text
ontok ontology overview --project anspire_excl_r115 --version 4
  业务总览
  83 个对象 → 8 个业务概念（+ 0 个未归组对象）
  …「管什么：客户主档、账号绑定关系、…」
overview exit = 0
ontok ontology overview --project anspire_excl_r115 --version 4 --json
overview --json exit = 0
```

（C 轮之前这一条是 `⚠ 客户视图泄露了建模术语（出口 C4）：["key"]`、exit 2。）
同一条 `scope_hint` 同时喂饱图与总览，而**导入闸**保证它客户安全。

### 4.4 审核界面仍能看到 `scope_hint`

`GET /api/review` 是内部轨（不过客户闸），条目 `payload` 原样给出：

```text
item kind=add_concept_group target=API 密钥与授权
payload.scope_hint = 「…以及未持 Key 的试用请求。…」（导入前的原文）
payload keys = display_name,evidence,extracted_by,key,members,scope_hint
```

---

## 5 · 页面 / 折叠（第 1 轮交付，本轮只恢复组说明一行）

| 功能 | 位置 |
| --- | --- |
| 组圈 + 组名 | `buildScene` 的 `g.gh` + `paint` 每帧算圆心/半径 |
| 折叠 / 展开 | `FOLD` + `unitsOf`（`:418`）+ `setFold`（`:1076`）+ `foldAll`（`:1089`）+ `relayout`（`:983`） |
| 聚合节点「组名 + N」 | `relayout` 拼 label，id `@g:<组 id>`；点它展开 |
| 组面板 | `showGroups`（`:1139`）/ `showGroup` / `locateMember`（`:1184`）/ `scope_hint`（`:1160`） |
| 组内聚拢 | `physicsStep` 里 `GRP_F`（`:300` / `:362`） |
| 无组如实 | `updateGroupInfo`：`GROUPS.length === 0` ⇒ 「未分组」，零聚合节点 |

折叠态节点数（`node tests/fixtures/req115/group_probe.js`，exit 0）：

```text
ALL PASS（折叠态 7 = 组 3 + 未分组 4）
```

REQ-114 布局探针复跑：`ALL PASS（168 帧 / 余量 15.79px / 5.20px）`。

---

## 6 · 折叠不改数据（AC-009）

折叠是纯前端内存状态（`FOLD`），`setFold` / `foldAll` / `relayout` 函数体里没有
`fetch(` / `ask(` / `api/`（探针断言）。同一版前后各导出一次：

```text
ttl1 = ttl2  = ACA259D906FA3819E847F313A8C0427DA3D3D6C7182BB6D287B67BBC2EDA6CB9
show1 = show2 = FD2A916A568960B3E5A834CE28A98A172BFA7DE5CC74D55B98AE0F504E09FDF7
```

---

## 7 · 测试与绕行

### 7.1 数字

```text
lib   291 passed / 0 failed   （272 → 288 → 290 → 本轮 291：N-2 的复合词正反两面）
doc     4 passed / 0 failed
集成   75 个目标跑完 · 802 passed / 3 failed（3 条既有红灯见 §7.3）
       另 1 个目标 tests/test_retained_names_req084.rs 因 Windows 绕行未编译（§7.2）
       新增 tests/test_req115_groups.rs —— 13 passed / 0 failed
       新增 tests/test_req115_import_gate.rs —— 2 passed / 0 failed（真进程 · N-1）
```

| 套件 | 结果 |
| --- | --- |
| `test_req115_groups`（13 条） | 13 passed |
| `test_req115_import_gate`（**新增 2 条 · 真进程**） | 2 passed（§9.1） |
| `test_req107_pipeline_bytes`（H1 重锚 + 其余 12 份） | 1 passed |
| `test_req107_zero_visible_change`（SHA256SUMS 互校） | 3 passed |
| `test_req107_source_enumeration`（写点/函数/模块登记） | 10 passed |
| `test_req107_outlet_pipeline`（判据直调只许在管线/守门器） | 12 passed |
| `test_graph_server`（页面守门 / 零外链 / 域层零 I/O） | 9 passed |
| `test_req114` / `test_req115` Node 探针 | `ALL PASS` |

> 口径：`lib 272 → 291` 只增不减；`doc 4` 不变；集成 76 个文件 = 75 跑 + 1 绕行。
> 没有任何既有用例被删 / 被 `#[ignore]` / 被放宽。

### 7.2 Windows 绕行双证

`src/bin/ontok-gate-preflight.rs` 是 unix-only（`std::os::unix::fs::MetadataExt`），
Windows 上编不过。按验收方式挪开，**finally 挪回**：

```text
sha256(before) = c7cf95e2a73b8c75bb44b1970396aca40f9146992d0c7dccb3636b3ba571322d
src/bin/ontok-gate-preflight.rs  Test-Path = False   （测试期间）
…
src/bin/ontok-gate-preflight.rs  Test-Path = True    （跑完挪回，134877 字节）
sha256(after)  = c7cf95e2a73b8c75bb44b1970396aca40f9146992d0c7dccb3636b3ba571322d
```

`git status --short` 无 `D` 行。代价：`test_retained_names_req084.rs` 用 `include_str!`
引那份源文件 ⇒ **该套件编不过**（与 REQ-113/114 同一口径，既有红灯）。

### 7.3 既有红灯（与本需求无关）

```text
drop_rate            → 缺 .review-workspace/fix-props/schema.json（fail-closed 输入不在版本控制内）
test_fk_naming       → 同一份脱敏 schema 缺失
test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract
                     → REQ-108 分档路由带来的既有 CLI 端到端差异（REQ-112/113 已登记）
test_retained_names_req084 → Windows 绕行导致编不过（同上，既有）
```

---

## 8 · 已知限制 / 下一轮 / 仍需裁定

### 8.1 设计缺陷（**已登记，下一轮拆字段**）

> **一个字段两种用途**：`scope_hint` 既是**客户可见的组说明**（总览 `summary`、图上组说明），
> 又被当成**内部备注**写（`suggest` 原来的「算法建议…待人审」）。

- 本轮缓解：C-1（导入时过客户闸）+ C-2（`suggest` 不写客户字段）；
- 根本修法：拆成 `scope_hint`（客户可见，过闸）+ **`review_note`**（内部轨，不过闸）。
  要动 REQ-010 的 schema 与快照，**不在本轮**；
- ⛔ 下一轮别再靠「某个出口删字段」来止血 —— 那只是把同一个病搬到另一个出口。

### 8.2 仍需裁定：`groups[].key` → 线上 `id`

AC-001 字面写 `groups: [{key, display_name, members[], reason?}]`，交付的是 `id`。
`key` 是发布闸词表里的词，写成线上键名会让每次带分组的 `/api/graph` 被拒（500）。
可选只有：改词表（红线禁）、叫 key 并接受 500、叫 id（`Node` 已有先例）。
本轮维持第三条，并留了正反判据。**若 QA 要求逐字，请裁定改 AC 或放开 guard()。**

### 8.3 AI 声明的处理方式（需用户执行一步）

AI 那份声明的 `api_entitlement.scope_hint` 需要人把「未持 Key 的试用请求」改成
「未持凭证的试用请求」（或任何客户安全的措辞），然后：

```text
ontok concept digest --file <file>     # 拿到新指纹
# 把 history[-1].content_digest 改成新指纹
ontok concept import --file <file> --project … --base-version …
```

**我们不替作者猜业务措辞**：报错已经点名到「组 + 字段 + 词条」；
静默改写等于替声明作者做命名决定（也是本项目一直禁止的 fail-open）。

### 8.4 其余已知限制（都已写进模块文档）

1. **组成员只算图里看得见的**：真库现场 84 实体 → 83 节点；同名对象只顾第一个（REQ-093 L2）。
2. **未做真浏览器手测**：组圈观感、折叠动画、组内引力手感只有源码级 + Node 探针。
3. **`scope_hint` 不做 AI 命名**（需求「不做」列表）；它由声明作者写。

---

## 9 · 第 3 轮 · QA 复审补项（N-1 / P1-1 / N-2）

### 9.1 N-1（**阻断**）：导入闸的接线用**真进程**钉住

QA 实测：把 `src/cli.rs` 的 `scan_group_names` 调用点删掉（函数留着），
`lib 290` 与 `test_req115_groups 13` **全绿** —— 与 REQ-114「探针数文本不看行为」同一类病。

**新测试**：`tests/test_req115_import_gate.rs`（2 条，spawn 真 `ontok concept import`）：

```text
① dirty_scope_hint_is_rejected_by_the_real_import_process
   声明：scope_hint = "…未持 Key 的试用请求"
   → exit 1
   → stderr: 组 `g1` 的 scope_hint 含建模术语 `key`
   → proposals(project) 仍 = 0（校验在碰库之前）

② clean_declaration_is_imported_by_the_real_import_process
   声明：scope_hint = "…未持凭证的试用请求"
   → exit 0 · stdout "共 1 个概念组"
   → proposals(project) = 1（真的落库，防「通过」假绿）
```

**变异实测（有牙）**：把 `src/cli.rs` 的闸调用整段注释掉，再跑 ①：

```text
test dirty_scope_hint_is_rejected_by_the_real_import_process ... FAILED
assertion `left == right` failed: 脏 scope_hint 必须 exit 1（否则闸根本没接线）
stdout=概念声明 v1.0.0 已导入提案 p1（项目 t_req115gate_dirty_… · 基于 v1）—— 共 1 个概念组，待双轨审核
  left: Some(0)
 right: Some(1)
test result: FAILED. 1 passed; 1 failed
```

恢复调用点后 `2 passed; 0 failed`（`src/cli.rs` 用 `git checkout` 还原，无残留标记）。

### 9.2 P1-1：边界写准确（**不是回归，但旧数据仍 500**）

现场：`anspire_excl_r115` **v3** 是 C-1 之前入库的脏快照（`scope_hint` 含「未持 Key」），
它仍然 `GET /api/graph → 500` / `ontology overview → exit 2`（本轮实测 v3 = **500**，v4 = 200）。
C-1 只挡**新导入**，读路径没改。

**改前**（`src/domain/graph.rs` `Group` 文档）：

```text
/// 所以图上带它（与总览口径一致，C-3）。它之所以安全，是因为
/// `concept import` 会先用客户发布闸判它（C-1：命中即拒不导入）——
/// 不会出现「快照里有脏说明、出图时整份 500」。
```

**改后**：

```text
/// - **新导入**的声明不会把脏说明带进来 —— `concept import` 会先用客户发布闸判它
///   （C-1：命中即拒不导入，报「组 + 字段 + 词条」）；
/// - **已入库的旧快照仍按原行为拒发**（`/api/graph` 500、`ontology overview` exit 2）——
///   读路径本轮没改，C-1 只挡新导入。要清理旧数据只能**重新导入**（改措辞、重算 digest）
///   或**重建快照**；⛔ 别把这条写成「已封堵」。
```

同步改：`Group::display_name` 的注释（「坏组名在导入时被拒」→ 只对新声明成立）、
`graph.rs` 的 L5 边界段、`tests/fixtures/req107/golden/PROVENANCE.txt`（同样写明
「新导入」与「旧快照仍拒发 + 清理方式」）。

### 9.3 N-2：词元级匹配（**保持现状，不改**）

`scan_modeling_terms` 按「非字母数字/下划线」切词元、且 `_` 是词内字符 ⇒
`api_key_group` 是一个词元 ≠ `key`，**不拒**；实测它也不会打挂图（200）。

**裁定：保持现状**（导入闸与出图闸是同一把尺子 ⇒ 过得了 import 就不会 500）。
⛔ 不改「含 key 字样就拒」——那会误伤 `api_key` / `license_key` 这些正常业务词。

文档已写进 `src/domain/concept.rs::scan_group_names` 的「⚠️ 匹配是**词元级**的」一节，
并加了单测 `test_compound_words_with_key_are_intentionally_not_hits`（正反两面：
`api_key_group` / `license_key` / `order_key_ref` / `keyring_service` 放行；
单独成词的 `Key` 仍必须被抓）。