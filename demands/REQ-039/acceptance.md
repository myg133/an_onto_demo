# 验收标准：REQ-039 测试二进制内 DDL×FK 写入并发的 `40P01` 死锁 hazard

> 依据 REQ-038 验收带出（QA 实测）写成。基线：`896c43c`（develop）。
> **非阻断项，但确实存在**，且它让每一次全量测试都有概率红，污染后续所有需求的验收信号。

---

## 背景：问题是什么

`tests/test_snapshot_write_paths.rs` 里的
`test_backfill_is_deterministic_preserves_and_idempotent` 会**额外跑 3 次**
`common::migrate()`，每次 `migrate()` 执行 3 条
`ALTER TABLE ... ADD COLUMN IF NOT EXISTS`。

**关键点：即使列已存在，`ALTER TABLE` 仍然要取 `AccessExclusiveLock`** ——
`IF NOT EXISTS` 只省掉「列不存在时报错」，不省掉「打开并持有排他锁直到事务提交」。

同一二进制里其它测试正在 `INSERT` 带外键的行，外键检查要取 `RowShareLock`。
两者并发 ⇒ 经典的锁环，实测报 `40P01 deadlock detected`。

**这在基线上就会发生**：QA 在基线 `9cc7613` 上连跑 6 轮，红了 2 轮。
所以它**不是 REQ-038 引入的**。

REQ-038 的处置只是「不往那口锅里加水」（把 2 条 DB 判据拆到独立二进制
`tests/test_snapshot_freeze_gate.rs`），**底层 hazard 没有修**。

> 注意：本条**不依赖 DBA**、**不依赖集群治理**（那是 REQ-031）。
> 这是测试代码自身的问题，本仓自己就能修。

## 死锁机制（为什么这三条方向都可能有效）

| 方向 | 机制 | 代价 |
| --- | --- | --- |
| ① `migrate()` 幂等短路 | 已存在的列**不进 DDL**，从源头消掉多余的排他锁 | 需正确判断「列存在」，判断错会导致表结构不完整 ⇒ **fail-closed** |
| ② 测试连接池串行化 | DDL 与数据写入**不并发**，锁环根本不成立 | 测试变慢；粒度太粗会拖垮全量时长 |
| ③ `lock_timeout` | 拿不到锁就**主动失败**而非死等，把不确定的等待变成确定的红 | 需要配合理阈值，否则误杀正常并发 |

**三者可选一或组合。** 具体方案由 Dev 判断，但必须说明为何选它、
以及它对「测试变慢多少」的影响。

---

## AC-001 · 死锁不再复现

- 测试步骤：对 `tests/test_snapshot_write_paths.rs` 跑**至少 10 轮**
  （基线上 6 轮红 2 轮，约 33% 复现率；10 轮应足以暴露残留）
- 预期结果：**0 轮**出现 `40P01 deadlock detected`
- 类型：功能
- **必须给出每轮的通过/失败明细**，不许只报「跑了很多轮」
- **不通过的处理**：若做不到 10 轮全绿，须说明实际复现率降到多少、
  剩余概率归因于什么（是本仓代码还是共享集群负载）

## AC-002 · 修复不是靠「关掉测试」

- 测试步骤：审 diff
- 预期结果：**没有删除或 `#[ignore]` 掉任何既有测试**
- 特别检查 `test_backfill_is_deterministic_preserves_and_idempotent` ——
  它是**死锁的来源之一**，但它本身测的是真实性质，**不能动它**
- 类型：架构
- 理由：删掉报错的测试是最省事的「修复」，也最没用 ——
  它抹掉的是**真信号**（测试确实不稳定），不是噪声

## AC-003 · 方案对「首次建库」路径同样成立

- 测试步骤：针对**空 schema**（首次 `migrate()`）验证
- 预期结果：空库首次迁移仍然把所有表建齐，不因幂等短路而漏建
- 类型：功能
- **理由**：方向 ①「已存在的列不进 DDL」的最大风险就是
  **空库上误判为「已存在」而漏建**。必须专门验证空库路径

## AC-004 · 并发判别力（变异自证）

- 测试步骤：对所选方案做变异，至少覆盖：
  1. 把幂等短路去掉（若选方向 ①）
  2. 把锁范围缩小（若选方向 ②）
  3. 把 `lock_timeout` 调到极大值（若选方向 ③）
- 预期结果：每条变异**真的跑过**，报告杀死它的判据的**二进制名 + 运行条数**
- 类型：测试有效性
- **存活变异必须逐条论证**：是「等价变异」还是「判据本身有洞」？

> **本条对本需求是个已知难点**：「死锁不复现」这类判据天然难做变异自证 ——
> 因为死锁本身是概率性的。QA 在 REQ-038 验收时已确认
> **必须「先跑自己的基线、再跑修复后」对比**，单边绿不能证明是修复起了作用。
> QA 若判定某条变异无法形成有效判据，须**明确说明理由**，不许含糊放过。

## AC-005 · 全量测试不劣化

- 测试步骤：至少 1 轮完整 `cargo test --no-fail-fast`，报耗时
- 预期结果：**521 passed / 0 failed / 1 ignored**（与 REQ-038 合并后基线一致）；
  若新增测试则总数相应增加
- **耗时**：本轮基线约 4~5 分钟。若方案 ② 导致显著变慢（如超过 10 分钟），
  须明确说明并给出理由 —— 这是可接受的取舍，但要写清楚
- 类型：回归

## AC-006 · 红线未被破坏

- 预期结果：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs`
  **零改动**；`test_humanize_still_refuses_unknown_columns` 一条断言都没放宽
- 类型：架构

## AC-007 · 产物未变

| 产物 | 基线 sha256 |
| --- | --- |
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` |
| `proposal.json`（`--project extracted`） | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` |

- 注：抽取命令实属 **`ontok-propose`**（`ontok extract` 不存在 —— 这是 REQ-038 验收
  查出的文档瑕疵 F5）。需显式 `--schema /home/myg133/Projects/Anchnet/Ontology/code/out/e2e-schema.json`，
  `--out` 在 `--stats` 之前

---

## 环境事实

- **共享集群 DB 很慢**：`SELECT 1` p50 实测 **57 ~ 170 ms**（正常 <5 ms）。
  **先探延迟再判断测试慢不慢。**
- **死锁率随集群负载浮动** —— 本需求的核心判据 AC-001 是个概率判据，
  跑测时**必须记录当时的集群状态**，否则复现率不可比。
- 报告必须把「代码失败」与「环境类失败」（`Connection reset by peer`、
  `40P01`）**分开写**，不声称稳定性能数值。
- 共享本体库**禁 DELETE / TRUNCATE / 清空表**；源库只读
  （仅 `information_schema` / `pg_catalog`）。
- 口令只走环境变量，不入库、不进文件、不进 commit message。

## 红线清单

| 文件 / 约束 | 说明 |
| --- | --- |
| `tests/drop_rate.rs` | 禁改 |
| `tests/test_fk_naming.rs` | 禁改 |
| `src/domain/view.rs` | 禁改 |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| `test_backfill_is_deterministic_preserves_and_idempotent` | **不得删除/ignore** —— 它测的是真性质 |
| `.review-workspace/fix-props/schema.json` | **绝不可删**（门禁输入） |
