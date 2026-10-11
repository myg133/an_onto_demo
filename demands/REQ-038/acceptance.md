# 验收标准：REQ-038 `set_extracted_by` 补冻结闸 + 判据注释修正 + 顺序断言

> 本文件依据 **REQ-033 验收带出的三条非阻断跟进**（见 `BA/dispatch/req-registry.md` REQ-038 行）写成，
> **不是依据 Dev 的实现倒推**。Dev 自述的任何数字都不作为验收依据。
> 基线：`9cc7613`（develop）。红线清单见文末。

---

## 背景：为什么这是问题

`PgStore::set_extracted_by`（`src/db/snapshot_store.rs`）在基线上**没有任何冻结检查**，
直接把 `update_version_extracted_by(...).execute(&self.pool)` 打到连接池上。

后果链条：

1. AC-006 要求「版本已冻结则拒绝写入」。
2. ADR-001 规定 `extracted_by IS NULL` = **人工修订**。
3. ⇒ 对一个**已冻结**的版本调 `set_extracted_by(version_id, &None)`，
   就能把它抹成 NULL，**等于伪造「这版有人工修订成分」**。

**非本次引入**（父提交同样无闸）。但今天**零生产调用方**（只有 7 处测试在用）⇒ 现在补最便宜。

---

## AC-001 · 冻结版本的 `extracted_by` 不可被改写

- 测试步骤：构造一个 `frozen_at IS NOT NULL` 的版本，调用 `set_extracted_by`
- 预期结果：
  - 返回**错误**，且错误类型与 `save_snapshot` 的冻结检查一致（`VersionFrozen { seq }` 一类，
    **不是** `RowNotFound` —— 版本存在，只是被冻）
  - **数据未变**：`extracted_by` 保持冻结前的取值
- 类型：功能
- **不通过的处理**：本需求的核心。不通过即 FAIL，无条件退回。

## AC-002 · 查与写在同一事务内（无 TOCTOU 窗口）

- 测试步骤：读代码 + 变异测试（见 AC-004）
- 预期结果：冻结检查与 UPDATE 处在**同一个事务**里；
  「池上先查 + 池上后写」的实现**不通过**
- 理由：后者把两个语句拆成两次自动提交，中间留窗口 —— 并发 `freeze()` 落在两者之间，
  照样写出「查时未冻结 / 写时已冻结」
- 类型：架构

## AC-003 · 所有能改写 `extracted_by` 的路径都受闸门保护

- 测试步骤：全仓搜出**每一条**能 UPDATE `extracted_by` 列的路径
  （`set_extracted_by`、`update_version_extracted_by` 的其它调用方、任何裸 SQL、
  `save_snapshot` 的其它分支、CLI import 路径），逐条列出是否受保护
- 预期结果：**零条旁路**。有一条没覆盖即 FAIL
- 类型：架构
- **理由**：本需求的根因是「有路径能绕过」，只堵一条不算堵住。

## AC-004 · 顺序断言有判别力（变异自证）

- 测试步骤：至少做以下变异，每条**真的跑过**测试：
  1. 摘掉冻结闸 → 须被杀死
  2. 闸门移到 `delete_snapshot_rows` 之后 → 须被杀死
  3. 闸门改成「池上先查 + 池上后写」 → 须被杀死
  4. 闸门与写拆成两个事务 → 须被杀死
- 预期结果：每条变异报告**杀死它的判据的二进制名 + 实际运行条数**
- 类型：测试有效性
- **存活变异必须逐条论证**：是「等价变异」（单线程下行为确实不可区分）还是
  「判据本身有洞」（存在能区分的场景：并发 / 错误码 / 数据残留）？后者为**阻断**。
- **不许用「等价变异」一句带过。**

## AC-005 · 判据注释方向与实现一致

- 测试步骤：独立验证「判据到底看不看得见块注释里的抄写」——
  造一个含块注释抄写的 fixture 直接试判据，不采信任何行数统计
- 预期结果：注释描述的行为与实现**一致**
- 已知事实（旧注释方向写反）：
  - `/* */` 块注释里的抄写**会被计入** ⇒ 假阳性
  - `//` 行注释里的抄写**看不见** ⇒ 漏报，**真盲区**
  - 而本仓 `//` 行注释**大量存在** ⇒ 漏报是真问题
- 类型：文档正确性
- 注：判据逻辑本身**不要求改**，只要求注释说真话。

## AC-006 · 红线未被破坏

- 测试步骤：`git diff 9cc7613..HEAD --stat` 逐个核对
- 预期结果：以下文件**零改动**：
  - `tests/drop_rate.rs`
  - `tests/test_fk_naming.rs`
  - `src/domain/view.rs`
- 且 `test_humanize_still_refuses_unknown_columns` **一条断言都没有放宽**
- 类型：架构

## AC-007 · 测试全绿且数字对得上

- 测试步骤：`cargo test --no-fail-fast`（至少 1 轮完整）
- 预期结果：0 failed；总通过数 = 基线通过数 + 本需求新增用例数
- 类型：功能

## AC-008 · 抽取产物未变

- 测试步骤：跑抽取命令，比对 sha256
- 预期结果：

| 产物 | 基线 sha256 |
| --- | --- |
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` |
| `proposal.json`（`--project extracted`） | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` |

- 类型：回归
- 注：本需求只碰写路径的冻结检查，抽取逻辑不该有任何变化。
  抽取命令需显式 `--schema /home/myg133/Projects/Anchnet/Ontology/code/out/e2e-schema.json`，
  且 `--out` 在 `--stats` **之前**。

---

## 环境事实（写给验收者，避免误判为代码失败）

- **共享集群 DB 很慢**：`SELECT 1` p50 正常应 <5 ms，实测在 **57 ~ 170 ms** 区间波动。
  **先探延迟再判断测试慢不慢。**
- 报告必须把「代码失败」与「环境类失败」（`Connection reset by peer`、
  `40P01 deadlock detected`）**分开写**，不声称稳定性能数值。
- 共享本体库**禁 DELETE / TRUNCATE / 清空表**；源库只读（仅 `information_schema` / `pg_catalog`）。
- 口令只走环境变量，不入库、不进文件、不进 commit message。

## 红线清单

| 文件 | 约束 |
| --- | --- |
| `tests/drop_rate.rs` | 禁改 |
| `tests/test_fk_naming.rs` | 禁改 |
| `src/domain/view.rs` | 禁改 |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json` | **绝不可删**（门禁输入，已丢过一次） |
