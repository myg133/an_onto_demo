# REQ-038 QA 独立验证报告

- 验证对象：Dev 提交 `696aa9f`（分支 `feature/REQ-038`；验证 worktree `verify-REQ-038`）
- 基线：`9cc7613`（develop）
- 验证时间：2026-10-09
- 报告路径说明：任务书要求报告写在 worktree 内的 `.review-workspace/verify-req038/`，但该路径不被任何 `.gitignore` 覆盖，写入后 `git status` 必脏，与「worktree 恢复到干净状态」的硬性要求冲突。故改写到根 `.review-workspace/verify-req038/`（任务书指定的临时区）。

---

## 1. 结论

**VERDICT: PASS 带非阻断项（建议合并到 develop）**

### 阻断项

**零条。** AC-001 ~ AC-008 全部满足；4 条必需变异 + 1 条补充变异全部被杀死；产物 sha 逐字节一致；红线零改动。

### 非阻断项

| # | 事项 | 依据 | 处置建议 |
|---|---|---|---|
| N1 | 交付注释里「基线 `9cc7613` 连跑 5 轮全绿」**与实测不符** | 我在基线上 6 轮里 **2 轮死锁**（run 3、run 4）。见 §6 | 改注释。事实性错误，但不改变处置结论（底层 hazard 既有、拆分方向正确） |
| N2 | `code_text()` 的「漏报真盲区」在本仓当前形态下实际危害为零，机制上仍是真盲区 | §3 实测 | 保持现状注释即可；已如实记录边界，符合 AC-005「只要求注释说真话」 |
| N3 | 底层 `test_backfill_…_idempotent` 的 DDL×FK 死锁 hazard 未修 | §6 基线 6 轮 2 红 | 另开需求。Dev 已如实标注未修 |
| N4 | 源码注释里「6762 行行注释」是基线数字，HEAD 实际 6791 | §3 | 数字过时但不影响结论方向 |

---

## 2. 逐问题回答

### Q1 · 冻结闸真的挡住了吗？—— 挡住，且数据零变化（独立探针证实）

没有采信 Dev 的测试。我另写了独立探针（验证后已移除），与 Dev 的测试不共用断言写法，关键差异是：**绕过 store 的反序列化层，直接查库比对原始列值**。

探针实际输出（HEAD `696aa9f`，实跑）：

```
QA_PROBE before freeze: seq=Some(1) eb=true
QA_PROBE set_extracted_by(None) on frozen => is_VersionFrozen=true seq_in_err=Some(1)
QA_PROBE after rejected write: seq=Some(1) eb=true
QA_PROBE VersionFrozen display = 版本 1 已冻结，不可写入（冻结是判据，规格 §9.1）
QA_PROBE unfrozen write eb=true
QA_PROBE save_snapshot on frozen => Err(VersionFrozen { seq: 1 })
QA_PROBE all assertions passed
```

证据要点：
- 冻结后写 NULL ⇒ `VersionFrozen`，且 `seq` 来自库中真实 seq（`Some(1)`），不是硬编码
- 写入前后直接查库读回的 `(seq, extracted_by)` **完全相等** ⇒ 数据未变（AC-001「数据未变」满足）
- 第二次写**非空**值同样被拒，数据仍不变 ⇒ 不是只挡 NULL
- **未冻结**版本仍能写 ⇒ 闸门没被写成「永远拒绝」（判别力前提，已独立复核）
- 对照组：`save_snapshot` 在同一冻结版本上同样 `VersionFrozen { seq: 1 }`，两者同形

**反向验证（证明该测试不是自证其说）**：在 M1（闸门整段摘掉）下跑**我的**探针，它红了，且抓到的是**真实数据损坏**而非错误码不符：

```
assertion `left == right` failed: 被闸门拒绝的写入改动了数据库内容 —— 这是阻断缺陷
  left: (Some(1), Some(Object {"agent": String("original-agent"), ...}))
 right: (Some(1), None)
```

即：无闸门时冻结版本的溯源被抹成 `None`（ADR-001 语义 = 伪造「人工修订」标记）—— 正是本需求的根因危害。判据有真实判别力。

### Q2 · 错误语义对吗？—— 对；根因路径已全封，零旁路

**(a) 与 `save_snapshot` 的冻结检查逐字对比**

`src/db/snapshot_store.rs:41-55`（`save_snapshot`）vs `:164-182`（`set_extracted_by`）：

| 分支 | `save_snapshot` | `set_extracted_by` | 一致？ |
|---|---|---|---|
| 版本不存在 | `Err(StoreError::Db(sqlx::Error::RowNotFound))` (:49) | `Err(StoreError::Db(sqlx::Error::RowNotFound))` (:176) | 逐字同形 |
| 已冻结 | `Err(StoreError::VersionFrozen { seq: snap.seq })` (:52) | `Err(StoreError::VersionFrozen { seq })` (:179) | 同形，`seq` 来源不同但语义正确 |
| 未冻结 | 继续 | 继续 | — |

`seq` 来源差异是必要且正确的：`save_snapshot` 有 `snap.seq` 可借，`set_extracted_by` 没有 `Snapshot`，所以自己查 `SELECT seq, frozen_at IS NOT NULL …`（:169）取回。已用探针验证 `seq` 与库中真实值一致。

对外文案一致：`版本 1 已冻结，不可写入（冻结是判据，规格 §9.1）`。

**(b) 根因路径全枚举（AC-003，零旁路）**

全仓**只有一条 SQL** 能写版本行的 `extracted_by` 列：
- `src/db/snapshot_store.rs:576` — `"UPDATE ontology_versions SET extracted_by = $2 WHERE id = $1"`

穷举核对（全仓 grep，无遗漏）：

| # | 路径 | 位置 | 受闸门保护？ | 论证 |
|---|---|---|---|---|
| 1 | `PgStore::set_extracted_by` | `snapshot_store.rs:137-199` | 本 REQ 新增 | 事务内查 `frozen_at` → `VersionFrozen` |
| 2 | `PgStore::save_snapshot` ④ 步 | `snapshot_store.rs:86` | 受保护 | `:41-55` ① 步闸门，`Some(true)` ⇒ 提前 `return Err`，**根本不走到 ④** |
| 3 | `write_snapshot_in` ③ 步（apply 路径） | `src/db/apply.rs:173` | 结构上不可达 | `pub(crate)`，全仓仅 `apply.rs:112` 一个调用方；调用点前一步 `apply.rs:99-107` 刚在同一事务里 `INSERT INTO ontology_versions (id,…)` 造出全新版本行，`next.version_id = new_version.0`（:108）。**版本是本事务刚 INSERT 的，不可能已冻结**。另 `apply.rs:60-62` 还有 `base 必须未冻结` 前置闸（`BaseVersionFrozen`） |

旁路排查（逐条排除）：

| 排查项 | 结果 |
|---|---|
| 裸 SQL 写该列 | 无。全仓 `SET extracted_by` 仅 `:576` 一处（`test_snapshot_write_paths.rs:368` 是 REQ-033 判据的 needle 字面量，非可执行路径） |
| `save_snapshot` 的其他分支 | 无其他分支，闸门在 ① 步统一前置 |
| CLI import 路径 | **不碰版本行**。`src/cli.rs:349/540` 调 `proposals.create(...)`，写的是 `proposals` 表（提案元数据），不是 `ontology_versions`；其后 `:662` 走 `apply_proposal` → 落到上表第 3 行（新建版本） |
| `INSERT INTO ontology_versions` | `project.rs:123`、`apply.rs:100` 两处，**均不写 `extracted_by` 列**（新版本初值即 SQL NULL，符合 ADR-001） |
| `UPDATE ontology_versions SET frozen_at…` | `project.rs:180` = `freeze()` 本身，只写 `frozen_at` |
| `proposals` 表的 `extracted_by` | 不同表不同语义（提案溯源），非本 AC 范围；ADR-003 已裁定 |

**结论：AC-003「零条旁路」成立。** 被闸门挡住后，不存在任何其它路径能抹掉冻结版本的 `extracted_by`。

### Q3 · 注释修正方向对吗？—— 对，已用 fixture 实测（不信行数统计）

按 AC-005 要求，我造了含块注释抄写的 fixture 直接试判据（探针验证后已移除），复刻 `code_text()` 的实现（`test_snapshot_write_paths.rs:376-385`）：

```
QA_PROBE block_comment_hits=1 line_comment_hits=0 real_code_hits=1
```

| 抄写藏在哪 | 判据看得见？ | 方向 | 旧注释 | 新注释 | 判定 |
|---|---|---|---|---|---|
| `/* */` 块注释 | **看得见，且被计入** | 假阳性 | 「看不见」错 | 「会被计入」对 | 新注释**正确** |
| `//` 行注释 | **看不见**（被 `filter` 掉） | 漏报（真盲区） | 「看不见」对 | 「看不见」对 | 两版都对 |
| 真实代码 | 看得见（对照，1 处） | — | — | — | 判别力前提成立 |

**Dev 的方向修正正确，旧文案方向确实写反。** 机制清楚：`code_text()` 只 `filter(|l| !l.starts_with("//"))`，对块注释一无所知 ⇒ 块注释内容留在文本里被计数；`//` 行注释整行被丢弃 ⇒ 看不见。

**行数统计核实（登记表 6842 vs Dev 6762 vs 实测）**：

| 口径 | 实测值 |
|---|---|
| `src/` 中**独占一行**的 `//` 注释（`^[[:space:]]*//`），**HEAD `696aa9f`** | **6791** |
| 同上，**基线 `9cc7613`** | **6762** |
| `src/` 中**任意位置**含 `//` 的行，HEAD | 6863 |
| 同上，基线 | 6834 |

结论：
- **Dev 说的「6762 行」= 基线 `9cc7613` 的数字**。HEAD 实际是 **6791**（本 REQ 自己新增约 29 行注释，把这个数改掉了）。数字过时但方向不变，且 Dev 引用的是真实测过的基线值 ⇒ **非阻断**（N4）。
- **登记表记的 6842（`BA/dispatch/req-registry.md:86`）与两者都不吻合**：既不是 HEAD 的 6791 独占行、也不是 6863 含尾注释、也不是基线的 6762/6834。属登记时的独立口径或旧快照，无法从当前仓库复现。**登记表数字应订正**，但不影响任何技术结论。
- 「`src/` 里真实块注释 0 处」—— **独立复核成立**：`grep -E '(^|[^*:])/\*'` 在 `src/` 仅 2 处命中，均为 `src/lib.rs:17` 与 `:22` 的 `// **不放 domain/**` —— 即行注释里的 markdown 加粗 `**`，**不是块注释**。真实块注释确为 0 处。

### Q4 · 顺序断言有判别力吗？—— 有；每条变异都真跑过

见 §3 变异表。**5 条变异全部实跑**，给出杀死它的判据的**二进制名 + 实际运行条数**。存活变异逐条论证见 §3 末尾。

### Q5 · Dev 声称的两条结论是否复核？—— 两条都复核，且都成立

**(a) M2 下「两条运行期判据存活、只有结构期判据杀死」—— 复现成立**

| 二进制 | M2 下结果 | 运行条数 |
|---|---|---|
| `test_snapshot_freeze_gate` | **存活**（2 passed / 0 failed） | 2 条全跑 |
| `test_snapshot_write_paths` | **杀死**（7 passed / 1 failed） | 8 条全跑 |

死因：`test_set_extracted_by_gate_and_write_share_one_transaction` panic 于 `:874`
「`set_extracted_by` 里没有开事务 —— 闸门与写入之间的 TOCTOU 窗口是真实的」。

我还用**自己的独立探针**在 M2 下跑了一遍 —— **同样存活（1 passed）**。即：运行期判据在单线程下**原理上**分不出这两种写法，不是 Dev 的测试写得弱。结构期判据不是冗余，是唯一能拦 M2 的东西。

**(b) M3 下既有运行期判据存活 3/3 —— 复现成立**

`test_freeze_gate_precedes_delete_and_delete_covers_five_tables` 在 M3 下**连跑 3 次全部 ok**（每次 1 passed / 7 filtered out）。

**掩盖机制我独立验证过**：`save_snapshot` 的两条错误路径都是 `return Err`，**没有 `tx.commit()`**（`snapshot_store.rs` 内 `commit()` 只出现在成功路径第 53 行）。闸门挪到 DELETE 之后时，`Some(true)` 分支直接返回 ⇒ `tx` 被 drop ⇒ **抢跑的 DELETE 随事务一起回滚** ⇒ 五张表旧行原样还在 ⇒ 运行期看到的「什么都没发生」与正确实现**完全一样**。回滚把次序问题掩盖了 —— Dev 的论证正确。

**注释是否真的改成了实测结论 —— 是。** `test_snapshot_write_paths.rs:815-830` 现文标题为「为什么要结构期判据钉顺序（**实测结论，不是推断**）」，正文写明「既有的运行期判据 …**抓不住顺序被调换**。REQ-038 实测（M3 变异…）：该运行期判据**单跑 3 次全部 ok** —— 存活」。

**残留自相矛盾表述排查**：全仓搜 `既有运行期判据|今天能挡住|顺序调换` ⇒ **0 命中**（grep exit=1）。旧的矛盾说法已彻底移除，无残留。

（注：注释里写结构判据「2/2 红」是 Dev 自己的轮次；我这边结构判据 1 次红，另有 1 条失败是环境死锁 —— 见 §4 拆分。结论方向一致。）

### Q6 · 拆分二进制的处置对吗？—— 根因属实，处置方向正确；但对基线的描述与实测不符（N1）

**(a) 死锁根因属实 —— 已独立核实**

`test_backfill_is_deterministic_preserves_and_idempotent` 内：
- `:38` 一次 `common::migrate(&pool)`
- `:64-65` `for _ in 0..3 { common::migrate(&pool); … }` ⇒ 循环内额外 3 次

合计额外 4 次 `migrate()`（Dev 说 3 次，指循环里的 3 次，口径略差但机制无误）。
`src/db/pg.rs` 每次 `migrate()` 含 **3 条 `ALTER TABLE … ADD COLUMN IF NOT EXISTS`**（`:242` `:248` `:299`）⇒ 额外 4 次 × 3 条 = **12 次 AccessExclusiveLock 申请**，与同二进制内其它测试的 FK 写入（RowShareLock）并发 ⇒ `40P01`。**根因描述准确。**

**(b) 基线是否也偶发？—— 会。基线 6 轮里红了 2 轮。**

在 **基线 `9cc7613`**（用 `code/` worktree，同命令同集群）跑 `cargo test --test test_snapshot_write_paths`：

| 轮次 | 结果 |
|---|---|
| 1 | ok 6 passed |
| 2 | ok 6 passed |
| **3** | **40P01 deadlock detected** |
| **4** | **40P01 deadlock detected** |
| 5 | ok 6 passed |
| 6 | ok 6 passed |

死锁 detail 与 Dev 描述**完全同型**（`AccessExclusiveLock` ↔ `RowShareLock` 互等）。

⇒ **Dev 注释里「基线 `9cc7613`：该二进制连跑 5 轮全绿」与实测不符（我这里 6 轮红 2 轮）。** 说明该 hazard 在基线上就已存在且并非罕见，不是被 Dev 的改动引入的。

**(c) 判断：可接受，不阻断**

- 底层 hazard 是**既有的**（基线 2/6 复现），**REQ-038 没有引入也没有恶化它**
- Dev 的处置是「不再往那口锅里加水」—— **方向正确**：把会 DDL + 插外键行的 DB 判据拆到独立二进制，利用「`cargo test` 逐个二进制串行、二进制内并行」的调度，让两者不再并发
- 代价是结构判据与运行期判据分居两个文件（可接受，二者本来职责不同）
- Dev **主动如实标注**了「底层那个 hazards 是**既有的**，本 REQ 没有动它… 这里只是**不往那口锅里再加水**」（`test_snapshot_freeze_gate.rs:24-26`）—— **未隐瞒**

**唯一问题是基线描述不准（N1）**：把基线说成全绿，会让后来人低估这个 hazard 的普遍性。但它**不改变处置结论**（基线也红 ⇒ 拆分仍是对的），故非阻断。

### Q7 · 红线未被破坏 —— 通过

`git diff 9cc7613..696aa9f` 完整改动面**仅 3 个文件**：

```
M  src/db/snapshot_store.rs
A  tests/test_snapshot_freeze_gate.rs
M  tests/test_snapshot_write_paths.rs
```

| 红线文件 | 结果 |
|---|---|
| `tests/drop_rate.rs` | 零改动（不在改动清单内） |
| `tests/test_fk_naming.rs` | 零改动（不在改动清单内） |
| `src/domain/view.rs` | **字节级相同**：基线与 HEAD 的 sha256 均为 `c0c2df2c99bd90c1e654050b66860a01721f428530da597bded4ad8502af56e6` |

`test_humanize_still_refuses_unknown_columns`（`src/domain/view.rs:1254`）**一条断言都没有放宽** —— 文件字节相同即证明（另单独核对：HEAD 与基线该测试体内的两条断言位于**相同的相对行偏移 8 与 30**）。

### Q8 · 产物未变 —— 两个 sha 逐字节一致

命令（`--out` 在 `--stats` 之前，按要求）：
```
cargo run --bin ontok-propose -- \
  --schema /home/myg133/Projects/Anchnet/Ontology/code/out/e2e-schema.json \
  --out <art>/proposal.json --stats <art>/stats.json --project extracted
```

| 产物 | 实测 sha256 | AC-008 期望 | 结论 |
|---|---|---|---|
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | 同左 | 一致 |
| `proposal.json`（`--project extracted`） | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | 同左 | 一致 |

抽取实跑成功：翻译覆盖 91/91 表、items 171 条、实体 91、关系 80、悬空 0。

（注：抽取命令属 `ontok-propose` 二进制，非 `ontok extract` —— 后者报 `unrecognized subcommand`，AC-008 描述略有出入。）

### Q9 · 全量测试

见 §4。**轮 1：521 passed / 0 failed / 1 ignored，35 个二进制，4m34s。** 与 Dev 自述**完全一致**。

---

## 3. 变异测试表

全部变异**真的运行过**。每条给出杀死它的判据的**二进制名 + 实际运行条数**。

| 变异 | 内容 | 结果 | 杀死它的判据（二进制名 + 运行条数） | 失败位置 |
|---|---|---|---|---|
| **M1** | `set_extracted_by` 的冻结闸门**整段摘掉**，退回裸 pool UPDATE | **杀死** | ① `test_snapshot_freeze_gate`（**2 条全跑**，0 passed / **2 failed**）<br>② `test_snapshot_write_paths`（**8 条全跑**，7 passed / **1 failed**）<br>③ 我的独立探针（**1 条**，**failed**，抓到真实数据损坏） | ① `:72` 冻结写未报 VersionFrozen（实际 `Ok(())`）；`:118` 未知版本未报 RowNotFound（实际 `Ok(())`）<br>② `:874` 「没有开事务」 |
| **M2** | 闸门改成「**池上先查 + 池上后写**」（有闸门但留 TOCTOU） | **杀死**（仅结构期） | `test_snapshot_write_paths`（**8 条全跑**，7 passed / **1 failed**）<br>**运行期 `test_snapshot_freeze_gate` 存活（2 条全跑，2 passed）** | `:874` 「`set_extracted_by` 里没有开事务」 |
| **M3** | `delete_snapshot_rows` 提到闸门**之前**（顺序调换） | **杀死** | `test_snapshot_write_paths`（**8 条全跑**，6 passed / **2 failed**，其中 1 条为环境死锁） | `:845` 「冻结闸门在第 45 行，「删旧行」调用在第 41 行」<br>**既有运行期判据存活 3/3**（`test_freeze_gate_precedes_delete_and_delete_covers_five_tables`，每次 1 passed / 7 filtered out） |
| **M4** | 摘掉两条结构期断言（`#[test]` → `#[ignore]`） | **存活**（预期） | 见下方逐条论证 | — |
| **M5**（我补做） | 闸门与写**拆成两个事务**（闸门事务先 `commit`，再开 `tx2` 写）—— **AC-004 第 4 条，Dev 未显式列为独立变异** | **杀死** | `test_snapshot_write_paths::test_set_extracted_by_gate_and_write_share_one_transaction`（**1 条**，0 passed / **1 failed**）<br>**运行期 `test_snapshot_freeze_gate` 存活（2 条全跑，2 passed）**；我的探针亦存活（1 passed） | `:883` 「版本行 UPDATE 没有在事务内执行 —— 「查在事务里、写在事务外」是半拉子，窗口照旧」 |

### 存活变异逐条论证

#### M4「摘掉两条结构期断言」存活 —— 不是等价变异，是「判据被移除」本身

**它不是变异源码，而是变异测试本身。** M4 问的是：如果有人把这两条结构断言摘掉，会怎样？答案分两层，我都实跑了：

1. **源码正确时摘掉断言** ⇒ 全绿。这本身无害（断言是保险丝，不是功能）。实测：M4-only 连跑 3 次，2 次 6 passed 全绿、1 次因环境死锁红。
2. **源码有缺陷（M3）且断言已被摘掉（M3+M4）** ⇒ **全绿，缺陷无人报警**。实测：M3+M4 组合下 `test_snapshot_write_paths` **8 passed / 0 failed**（拿到一个无死锁的干净样本）。

⇒ 结论：**这两条结构断言不是冗余，是 M3 这类缺陷的唯一探测器。** 摘掉它们 = 交出一个当前无人看守的洞。这正是 AC-004 要求它们存在的原因，**M4 存活是正确且预期的结果，不构成判据有洞**。

#### M2 / M5 在运行期判据下存活 —— 是「单线程下不可区分」，但绝不等于等价变异

必须严格区分二者。单线程、无并发插入时，M2/M5 与正确实现的返回值**确实一样**（都返回 `VersionFrozen`），所以运行期判据存活是**原理上**的，不是判据写得弱。

**但它们不是等价变异** —— 存在能区分的场景，而且我**把它跑出来了**（探针验证后已移除）：

```
QA_PROBE [M2] gate query saw frozen=Some(false)      ← 闸门查：未冻结
QA_PROBE [M2] post-gate write result = Ok(1)         ← 窗口内 freeze 落定后，仍写入成功
QA_PROBE [M2] agent after = Some("m2-wrote-anyway")  ← 冻结版本的溯源被改写！
QA_PROBE [correct] frozen write => Some("版本 2 已冻结，不可写入…")
QA_PROBE [correct] agent after = Some("original-b") (未变)
QA_PROBE 结论：M2 与正确实现**行为可区分**
```

**区分场景：并发 `freeze()` 落在「查」与「写」之间。**
- M2 形状：查（自动提交，未冻结）→ 窗口内 `freeze()` 落定 → 写（自动提交）⇒ **写入成功，冻结版本的溯源被改成 `m2-wrote-anyway`**
- 正确实现（同一事务）：冻结检查与 UPDATE 共享一个事务边界 ⇒ **拒绝，数据不变**

⇒ **数据残留可观测 ⇒ M2 不是等价变异。** 运行期判据的存活是「单线程盲区」，而**结构期判据恰好补上了这个盲区**。若没有 `test_set_extracted_by_gate_and_write_share_one_transaction`，M2/M5 会带着一个真实的溯源伪造洞合入。

**关于任务书点名要判断的那条**：「Dev 自己也承认：冻结闸目前**打在 `self.pool` 上**，查与写之间有窗口（这条要在报告里判断是否阻断）」——

**事实是：当前代码并没有把闸门打在 `self.pool` 上。** 实现为 `snapshot_store.rs:162` `begin()` → `:172` `fetch_optional(&mut *tx)` → `:194-196` `execute(&mut *tx)` → `:197` `commit()`，**全程在同一事务内**；且 `test_snapshot_write_paths.rs:887-891` 有断言明文禁止 `&self.pool` 在该方法体内复现。**TOCTOU 窗口不存在，该保留项非阻断。** Dev 描述的 M2 是它**刻意拒绝采用的备选方案**（注释 `:154-159` 写明了拒绝理由），不是当前代码的状态。

---

## 4. 测试与环境

### 全量测试数字

| 轮次 | 二进制数 | passed | failed | ignored | 耗时 | 备注 |
|---|---|---|---|---|---|---|
| **轮 1**（Dev 代码，无我的探针） | **35** | **521** | **0** | **1** | **4m34s** | 与 Dev 自述**完全一致** |
| 轮 2（含我的 4 个探针二进制） | 39 | 524 | **1** | 1 | 5m27s | 唯一失败 = 环境死锁，见下 |

**AC-007 核对**：基线 517 + 本需求新增 4 = **521**，核对通过。
- `tests/test_snapshot_write_paths.rs`：基线 **6** 条 → HEAD **8** 条（+2 结构期判据）
- `tests/test_snapshot_freeze_gate.rs`（新文件）：**2** 条（+2 运行期判据）
- 二进制数：基线 30 → HEAD 31（+1）
- 轮 2 算术自洽：521 + 我的 4 个探针 = 525 ⇒ 524 passed + 1 failed

**1 条 ignored**：`todo_identifier_defenses_have_own_assertions`（`full-run1.log:555`）—— **既有的 `#[ignore]`，与 REQ-038 无关**。

### 「代码失败」与「环境类失败」分开写

**代码失败：0 条。** 轮 1 全程 `0 failed`；轮 2 唯一失败的断言内容是**数据库返回的 `40P01`**，不是代码断言不成立。

**环境类失败：`40P01 deadlock detected`（间歇性，命中一个 REQ-038 未触碰的既有用例）**

| 出现场景 | 命中用例 | 性质 |
|---|---|---|
| 轮 2 全量 | `test_backfill_is_deterministic_preserves_and_idempotent`（`test_snapshot_write_paths.rs:623`） | **既有用例，REQ-038 diff 零改动**（`git diff` 中该函数 0 命中） |
| M3+M4 重试 | 同上 | 同上 |
| M4-only run 1 | 同上 | 同上 |
| **基线 `9cc7613` 6 轮中的 2 轮** | 同上 | **基线即复现 ⇒ 与 REQ-038 无关** |

死锁 detail（轮 2 原文）：
```
Process 3282579 waits for RowShareLock on relation 327302; blocked by process 3282577.
Process 3282577 waits for AccessExclusiveLock on relation 327350; blocked by process 3282579.
```

**我如何区分二者**：每轮都先 `grep -q "40P01"` 判定，若命中则**丢弃该轮重跑**（最多 8 轮），直到拿到一个无死锁的干净样本再下结论。所有「存活/杀死」判定**均基于无死锁的干净样本**，没有把环境失败误记成代码通过。

### 本轮实测 DB 延迟

探针 `SELECT 1` × 30（探针验证后已移除）：

```
QA_PROBE latency n=30 min=152551us p50=156303us p95=168033us max=238922us mean=159489us
```

| 指标 | 实测 | 任务书给的参考 |
|---|---|---|
| p50 | **156 ms** | 「正常应 <5 ms」 |
| p95 | 168 ms | 「实测在 57~170 ms 区间波动」 |
| max | 239 ms | — |

⇒ **本轮集群比任务书描述的区间更慢**（p50 156 ms，接近 57~170 ms 区间上沿）。这解释了为何全量一轮 4~5 分钟，以及为何死锁在基线也高频复现（DDL 与 FK 写入的锁等待窗口被拉长）。

**不声称稳定性能数值** —— 单次采样，环境共享，仅供判读「测试慢是环境而非代码」。

---

## 5. 验收标准逐条判定

| AC | 内容 | 判定 | 关键证据 |
|---|---|---|---|
| AC-001 | 冻结版本 `extracted_by` 不可被改写（报错 + 数据未变 + 非 `RowNotFound`） | **PASS** | 独立探针：`VersionFrozen{seq:1}`，写前写后直接查库完全相等 |
| AC-002 | 查与写同一事务，无 TOCTOU | **PASS** | `:162 begin` / `:172 fetch_optional(&mut *tx)` / `:194 execute(&mut *tx)` / `:197 commit`；M2/M5 均被杀死 |
| AC-003 | 所有能改写 `extracted_by` 的路径都受保护（零旁路） | **PASS** | 唯一 SQL `:576`，3 个调用点逐条论证；裸 SQL / CLI / save 其他分支 / INSERT 全部排除 |
| AC-004 | 顺序断言有判别力（4 条变异全杀死 + 存活逐条论证） | **PASS** | M1/M2/M3/M5 全杀死；M4 存活已逐条论证（非等价变异，是判据被移除本身） |
| AC-005 | 判据注释方向与实现一致 | **PASS** | fixture 实测 `block=1 / line=0`；旧文案方向确实反了，新文案正确 |
| AC-006 | 红线未被破坏 | **PASS** | 改动面仅 3 文件；`view.rs` sha256 字节相同 |
| AC-007 | 测试全绿且数字对得上 | **PASS** | 轮 1：521/0/1，35 二进制；517+4=521 核对通过 |
| AC-008 | 抽取产物未变 | **PASS** | 两个 sha256 逐字节一致 |

---

## 6. 发现但未修的问题（如实报）

| # | 问题 | 位置 | 影响 | 判断 |
|---|---|---|---|---|
| **F1** | 「基线连跑 5 轮全绿」与实测不符（基线 6 轮红 2 轮） | `tests/test_snapshot_freeze_gate.rs:17` | 事实性错误，会让后来人低估该 deadlock 的普遍性 | **非阻断**，建议改注释。基线也红 ⇒ 拆分处置仍正确 |
| **F2** | 底层 `test_backfill_…_idempotent` 的 DDL×FK 死锁 hazard 未修 | `src/db/pg.rs`（3 条 ALTER）× `tests/test_snapshot_write_paths.rs:38,64-65`（额外 4 次 migrate） | 间歇性测试红（本轮实测基线 2/6、HEAD 轮 2 1/1） | **非阻断，建议另开需求**。基线即存在，REQ-038 未引入；Dev 已如实标注未修 |
| **F3** | 「6762 行行注释」是基线数字，HEAD 实际 6791 | `tests/test_snapshot_write_paths.rs:402` | 数字过时 | **非阻断**。方向结论不变 |
| **F4** | 登记表「6842 处 `//` 注释」与任何可复现口径都不吻合 | `BA/dispatch/req-registry.md:86` | 登记表数字应订正 | **非阻断**，建议 BA 订正为「HEAD 6791 独占行 / 基线 6762」并注明口径 |
| **F5** | AC-008 写「抽取命令」易被读成 `ontok extract`，实际属 `ontok-propose` 二进制 | `BA/demands/REQ-038/acceptance.md` AC-008 | 验收者可能跑错命令 | **非阻断**，文档瑕疵 |
| **F6** | `code_text()` 的 `//` 盲区在本仓当前形态下实际危害为零，机制上仍是真盲区 | `tests/test_snapshot_write_paths.rs:376-385` | 将来若有人把抄写藏进 `//` 注释，判据看不见 | **非阻断**。AC-005 只要求「注释说真话」，Dev 已如实标注边界 |

---

## 7. QA 自己引入又已复原的问题（如实报）

| # | 事项 | 复原情况 |
|---|---|---|
| **S1** | 创建了 4 个探针测试文件（`tests/zz_qa_probe_{ac005,freeze,latency,toctou}.rs`）+ 1 个变异脚本 + 源码/测试备份，**全部在 verify worktree 内** | 4 个探针文件已用本机可恢复移除工具处理；worktree `git status --porcelain` = **0 行** |
| **S2** | 对 `src/db/snapshot_store.rs` 与 `tests/test_snapshot_write_paths.rs` 施加过 5 条变异（M1/M2/M3/M5 + M4） | 已从 sha256 校验过的备份还原。`src/db/snapshot_store.rs` sha256 = `930865434d6f517480aa09a20cbd67b44d950b68e1fa81442dde4611082b5d80`（与 pristine 备份一致）；`git diff --stat` **为空** |
| **S3** | verify worktree 原本**没有** `target/`（热 target 只在 `code/` 与 `feature-REQ-038/`） | 按「禁止 `CARGO_TARGET_DIR`」的纪律，用 `cp -a --reflink=auto` 把 `feature-REQ-038/target` **复制**进 worktree（5.3G），全程未设置任何 `CARGO_TARGET_DIR` 环境变量。**这是我自己引入的环境变更，如实报告** |
| **S4** | 第一次全量测试因 `.review-workspace/verify-req038/` 目录当时不存在，日志重定向失败（`EXIT=1`，44 秒空跑） | 补建目录后重跑，即轮 1。空跑未产生任何测试结论 |
| **S5** | 在 `verify-REQ-038/` 下建过 `.review-workspace/verify-req038/` 准备放报告 | 发现该路径不被 gitignore 会弄脏 `git status`，与「worktree 干净」硬性要求冲突，该目录已复原，报告改写根 `.review-workspace/verify-req038/` |
| **S6** | 在基线 `code/` worktree 上跑了 6 轮 `cargo test --test test_snapshot_write_paths`（Q6 要求） | 只运行测试，**未触碰任何文件**；`code/` 仍在 `9cc7613`，无 diff |
| **S7** | 硬性纪律遵守自查 | 未 push（仓库无 remote）、未 squash/force/改历史、未使用不可恢复的删除命令、口令只走 `ONTOK_TEST_DATABASE_URL` 环境变量（未入文件/日志/commit message）、临时文件只在 `.review-workspace/verify-req038/`、共享本体库只做 INSERT（探针只 SELECT + 建项目/版本/冻结，无破坏性清库操作）、`.review-workspace/fix-props/schema.json` mtime 保持 `2026-10-07 23:55:23` **未触碰** |

**worktree 终态**：`HEAD = 696aa9f`，`git status --porcelain` = **0 行**（完全干净）。

---

## 8. 合并建议

**建议合并到 develop。** 8 条 AC 全部满足，零阻断项。

合入时建议一并处理 **N1/F1**（改 `test_snapshot_freeze_gate.rs:17` 的基线描述为实测值）—— 一行注释的事实订正，可在本 PR 内顺手改掉，不必单开。**F2**（底层死锁 hazard）建议另开需求跟进，不阻断本次合并。
