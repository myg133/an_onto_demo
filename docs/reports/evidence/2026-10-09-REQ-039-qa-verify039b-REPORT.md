# QA 复验报告 · REQ-039 修复轮（`ab2f56c`）

> 被验对象：`feature/REQ-039` worktree，HEAD = `ab2f56cc9b2869e3e3b1a5ba5a045f22105b67a2`
> 上一轮结论：FAIL（解码缺陷 + 判据无判别力），已退回。Dev 修复轮自述 3 项改动。
> 本轮结论：**PASS 带非阻断**

---

## 0. 结论摘要

| 问题 | 结论 | 关键证据 |
| --- | --- | --- |
| Q1 解码真修好了吗 | **是** | 真函数 `patched_columns_present` 解出 **3** 个值；同一条 SQL、同一连接、同一时刻用旧解码法只解出 **1** 个 |
| Q1b 加第 4 条 ALTER 零改动 | **是** | 真加第 4 条 → 解码代码**一字未改** → `test_db` 11/0 通过 |
| Q2 形状不符报错吗 | **是**（3/3 方向） | 真函数返回 `sqlx::Error::Protocol`，**在任何 ALTER 发出之前** |
| Q3 判据有判别力吗 | **是** | M1 被杀死，红在 `table=action_types` 55P03，`ontology_versions` 通过 |
| Q4 死锁被消除还是测不出 | **两者都要说**（见 §4） | 概率口径**两侧都是 0**，不能区分；确定性口径**证明已消除** |
| Q5 全量 / AC-005 | **通过** | 522 passed / 2 failed / 1 ignored，304s；2 个失败均为**环境类**，单跑全绿 |
| Q6 红线 | **全部守住** | 3 个禁改文件 sha256 与 develop **逐字节相同**；无删除、无新增 `#[ignore]` |
| Q7 产物未变 | **通过** | 两个 sha256 **精确匹配** |
| Q8 两个决策项 | 认同 Dev（`search_path`）；REQ-040 建议现在就修 | `pg_temp` 遮蔽已实测复现，失败方向确为保守 |

**Dev 自述的三项改动全部属实，且本轮补做了他没做的两条单边变异。**

---

## 1. Q1 · 解码真的修好了吗 —— **是**

### 1.1 直接测**真私有函数**（不是复刻）

`patched_columns_present` 是私有函数，集成测试调不到。为测**真函数**而非复刻，
我在 `src/db/pg.rs` 尾部临时追加 `#[cfg(test)] mod qa_probe`（验完已删，sha256 已复原）。

```
QA_PROBE decoded_len=3 values=[true, true, true]
QA_PROBE ALTER_PATCHES_len=3
QA_PROBE raw_rows=1 raw_cols=3
QA_PROBE old_style_decoded_len=1
```

**同一时刻、同一条 SQL、同一条连接**上的对照组最有说服力：

| 解码方式 | 解出长度 |
| --- | --- |
| 修复后 `row.try_get::<bool,_>(i)` 循环 | **3** |
| 修复前 `query_as::<_, Vec<(bool,)>>` | **1** |

⇒ 原缺陷被原样复现（对照组），且修复后确为 3。

代码位置：`src/db/pg.rs:203`（取 `Row`）、`pg.rs:224-226`（列数守卫）、
`pg.rs:230-233`（`for i in 0..want { row.try_get::<bool,_>(i)? }`）、`pg.rs:6`（新增 `use sqlx::Row;`）。

### 1.2 「加第 4 条 ALTER 零改动」—— **属实，且比声称的更强**

我真加了第 4 条 `ALTER_PATCHES`（`("projects","name",...)`，数组 `[..; 3]` → `[..; 4]`），
**对 `patched_columns_present` 一个字都没改**：

```
cargo test --offline --test test_db -- --test-threads=1
→ test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 43.20s
```

这条比「跑通了」更强：`want = 4`，守卫要求 `got == want`，否则 `migrate()` 直接
返回 `Err(Protocol)`。**全绿 ⇒ 4 列确实被正确解出。**

| 位置 | 与 N 的关系 |
| --- | --- |
| `pg.rs:190-199` `sel` 生成 | `.map()` 遍历 `ALTER_PATCHES` ⇒ 列数自动 = N |
| `pg.rs:205` `want` | `= ALTER_PATCHES.len()` ⇒ 自动 = N |
| `pg.rs:224-228` 列数守卫 | 比较 `got` vs `want`，两边都随 N 走 |
| `pg.rs:231` 解码循环 | `for i in 0..want` ⇒ 自动解 N 列 |
| `pg.rs:102` 消费者 | `.zip()` ⇒ 自动配对 N 对 |

---

## 2. Q2 · 形状不符真的报错了吗 —— **是（3/3 方向）**

### 2.1 先说明「怎么触发」

SQL 由 `ALTER_PATCHES` 生成（`pg.rs:190-203`），**正常路径下 `got` 恒等于 `want`、
`rows.len()` 恒为 1**——§1.1 实测 `raw_rows=1 raw_cols=3` 正是这个恒等式。

⇒ **不破坏不变量就无法触发**，与题面预期一致。我的办法是：**临时注入 `sel` 生成与
`ALTER_PATCHES.len()` 的背离**——这正是守卫注释里写的「代码与库不同源」场景本身。
测的是**守卫在它被设计来防的那种损坏下会不会响**。

### 2.2 全部触发，且都在发 ALTER **之前**中止

| # | 注入 | 真函数 `migrate()` 返回 |
| --- | --- | --- |
| a | `sel.pop()`（2 vs 3） | `Protocol(… 期望 3 列（= ALTER_PATCHES 长度），实际 2 列 …)` |
| b | `sel.push(sel[0])`（4 vs 3） | `Protocol(… 期望 3 列 … 实际 4 列 …)` |
| c | `… FROM generate_series(1,3)` | `Protocol(… 期望 1 行（无 FROM 的 SELECT），实际 3 行 …)` |
| d | `sel.insert(0,"TRUE")`（**最前面** ⇒ 索引右移） | `Protocol(… 期望 3 列 … 实际 4 列 …)` |

守卫在函数**内部**返回，`migrate()` 的 `?`（`pg.rs:92`）立即传播 ⇒
**batch 根本没被构造，一条 ALTER 都没发出**。与上一轮「`unwrap_or(false)` 把看不懂
伪装成列不存在然后照发 ALTER」正好相反。

---

## 3. Q3 · 判据现在有判别力吗 —— **有（最重要的一问）**

所有变异**真的跑过**。变异通过临时改写 `src/db/pg.rs` 实现，跑完即从 `ab2f56c`
原版副本复原（sha256 逐次核对）。

### M1 —— 同时回退解码 **与** 消费者 ⇒ **被杀死**

```
cargo test --offline --test test_db -- --test-threads=1
→ test result: FAILED. 10 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 58.67s
failures:
    test_migrate_takes_no_access_exclusive_lock_on_settled_schema
panicked at tests/test_db.rs:572:9:
稳态库上 migrate() 没能跑完（table=action_types, Err(Database(PgDatabaseError {
  code: "55P03", message: "canceling statement due to lock timeout" ...
```

**红在 `table=action_types`；`ontology_versions`（索引 0，唯一被正确解出的那列）通过。**
与 Dev 自述一致 ⇒ 上一轮「只压 `ontology_versions` 就看不见另两条」的洞**确实补上了**。

### M3 —— **只**回退解码（留 `zip`）⇒ **被杀死，但杀死它的是另一条测试**

```
→ test result: FAILED. 10 passed; 1 failed; ... finished in 36.60s
failures:
    test_migrate_still_alters_when_table_exists_but_column_does_not
panicked at tests/test_db.rs:449:10:
旧库迁移失败 —— ALTER 短路把补列路径弄坏了:
  code: "42703", message: "column \"ordinal\" does not exist"
```

**判别力归属很关键**：这次锁判据 `…takes_no_access_exclusive_lock…` **通过了**。
因为 `zip` 遇长度 1 的 `present` 会**截断** ⇒「后两条 ALTER 被**跳过**」而非「发出」——
**相反方向**的缺陷（漏建，非多锁），锁判据天然看不见。Dev 的推理正确。

杀死它的是 `test_migrate_still_alters_when_table_exists_but_column_does_not`
（`tests/test_db.rs:409`）。⇒ **判据集合整体有覆盖，不是靠单条判据。**

### M4 —— **只**回退消费者（留正确解码）⇒ **存活**

```
→ test result: ok. 11 passed; 0 failed; ... finished in 37.74s
```

**逐条论证：货真价实的等价变异。** 解码正确 ⇒ `present.len()==3==ALTER_PATCHES.len()`
⇒ `get(i)`（i∈{0,1,2}）**永不越界** ⇒ `unwrap_or(false)` 的 fallback **永不可达**
⇒ 与 `zip` 逐对等价。行为不可观测差异，**判据没有洞**。

### M2 —— 去掉列数断言 ⇒ **存活**（Dev 判为「等价变异」）

```
→ test result: ok. 11 passed; 0 failed; ... finished in 40.29s
```

**我对「等价变异」这个标签有异议**，见下。

### 3.3 M2 不是等价变异（唯一实质异议）

Dev 理由：「`try_get` 循环本身已正确解出全部 N 列」。我**实测**了三种形状：

| 组合 | 真函数 `migrate()` 返回 | 说明 |
| --- | --- | --- |
| M2 + 末尾追加一列（4 vs 3） | **`Ok`** | 追加不移动索引 ⇒ 值仍正确 ⇒ **无害** |
| M2 + **最前面**插入一列（索引右移） | **`Ok`** ⚠️ | `try_get(0..3)` 读到**整体错位**的值 ⇒ **三张表的「列是否存在」全部答错，且不报错** |
| M2 + 少一列（2 vs 3） | `Err("column index out of bounds: the len is 2, but the index is 2")` | 仍 fail-closed，信息不友好 |

⇒ **M2 不是等价变异**：它在「列序错位」下留下**静默给出错误答案**的洞，
函数内部**没有任何东西能发现它**（`try_get` 只在越界时报错，不校验语义对应）。

**但不是判据的洞，也不是阻断项** —— 我把「M2 + 索引错位」整个丢进 `test_db`：

```
→ test result: FAILED. 10 passed; 1 failed; ... finished in 37.55s
failures:
    test_migrate_still_alters_when_table_exists_but_column_does_not
panicked at tests/test_db.rs:466:9:
`ontology_versions` 已在、`extracted_by` 不在，migrate() 却没有把它补上
```

**⇒ 这个洞被另一条判据（漏建方向）覆盖了。** 结论：M2 =「可接受的冗余层，删掉不改变
现有判据结论，但会在列序错位时静默给错答案」。**建议保留该断言**（成本为零），
并把 Dev 注释里「等价变异」的措辞改成如实描述。

### 3.4 变异测试表（汇总）

| 变异 | 内容 | 结果 | 运行条数 / 杀死它的判据 | 存活论证 |
| --- | --- | --- | --- | --- |
| **M1** | 回退解码 **+** 消费者 | **KILLED** | 11 条跑，`test_db`：**10P/1F**，58.67s。杀死者 `test_migrate_takes_no_access_exclusive_lock_on_settled_schema`（红在 `table=action_types`，`55P03`） | — |
| **M3** | **只**回退解码（留 `zip`） | **KILLED** | 11 条跑，`test_db`：**10P/1F**，36.60s。杀死者 `test_migrate_still_alters_when_table_exists_but_column_does_not`（`42703 ordinal`）。**锁判据本轮通过** | — |
| **M4** | **只**回退消费者 | **SURVIVED** | 11 条跑，`test_db`：**11P/0F**，37.74s | **等价变异**：解码正确 ⇒ `get(i)` 永不越界 ⇒ fallback 不可达 |
| **M2** | 去掉列数断言 | **SURVIVED** | 11 条跑，`test_db`：**11P/0F**，40.29s | **非等价**：索引错位时静默给错答案；但该组合被 `test_migrate_still_alters_…` 杀死（10P/1F，37.55s）⇒ **非判据之洞** |
| **M2+错位** | M2 复合前置插列 | **KILLED（复合）** | 11 条跑，`test_db`：**10P/1F**，37.55s | 证明 M2 残余洞有判据兜底 |
| **A4** | 真加第 4 条 ALTER | **存活（预期）** | 11 条跑，`test_db`：**11P/0F**，43.20s | 非变异，是 §1.2 的零改动验证 |

（`P`/`F` = passed/failed。二进制均为 `test_db`，命令均为
`cargo test --offline --test test_db -- --test-threads=1`。）

---

## 4. Q4 · 死锁是「被消除了」还是「测不出来」

**两个都要说，因为这是两个不同的问题，而本需求的核心恰恰在于把它们分开。**

### 4.1 概率口径：**确实测不出来**（Dev 的说法，我独立复现了）

| 口径 | 命令 | 运行数 | 通过 | 失败 | **`40P01` 次数** |
| --- | --- | --- | --- | --- | --- |
| AC-001 顺序 10 轮（修复后） | `cargo test --offline --test test_snapshot_write_paths -- --test-threads=1` × 10 | 10 | **10** | 0 | **0** |
| K=3 跨进程（修复后） | 预编译 `test_snapshot_write_paths-4119f8d8fad5cac3`，3 进程并发 × 5 轮 | 15 | 13 | 2 | **0** |
| K=3 跨进程（**M1 缺陷体**） | 同上，M1 变体重新编译 | 15 | 10 | 5 | **0** |

**修复体与缺陷体两侧 `40P01` 都是 0 ⇒ 该口径零判别力，Dev 说法成立。**
我的数字（13/15、10/15）与 Dev 自述（fixed 14/15、baseline 9/15）同量级但不完全相同
——这是概率判据 + 集群负载浮动，**不构成矛盾，也不声称稳定值**。

**K=3 全部 7 个失败（修复体 2 + M1 5）都是同一个 REQ-040 前提竞态，没有一个是死锁：**

```
tests/test_snapshot_write_paths.rs:680:5: 前提：迁移前历史行应当全是 DEFAULT 0
  left: [0, 1, 2]   right: [0, 0, 0]      （M1 侧 5 次全是这一种）
  left: [0, 0, 1]   right: [0, 0, 0]      （修复侧 1 次）
30 次 K=3 运行日志中 "40P01" 出现 0 次
```

⇒ **「0 次 40P01」只证明「没观测到」，不证明「已修好」。** 若只报这个数字，
它与「缺陷仍在、只是没被触发」在数据上**完全无法区分**。这正是 AC-004 预告的难点。

### 4.2 确定性口径：**证明死锁源已被消除**（概率口径给不出的那个结论）

真正的证据是 M1：

- **缺陷体（M1）** 在确定性判据下**变红**，红在 `action_types` 55P03
  ⇒ 它在稳态库上**确实还在对 `action_types` 发 ALTER**（确实在取排他锁）；
- **修复体** 在同一判据下**全绿**，且绿了 4 次（基线 38.03s / 加第 4 条 43.20s /
  变异复原 37.74s / 最终确认 38.13s）⇒ **一条 ALTER 都不发**。

**「零条 ALTER」⇒「零次 `AccessExclusiveLock` 请求」⇒「锁环的一端根本不存在」⇒
死锁在稳态 schema 上不是「没被触发」，而是「结构上不可能发生」。**

这是**反例证明**（构造一个会出错的版本证明判据能抓 ⇒ 抓不到的就真是对的），
不是概率观察。所以：

> **死锁是被消除了（就 `ALTER_PATCHES` 这三条在稳态 schema 上的作用面而言），
> 不是「测不出来」。**
> **但 AC-001 那个概率口径本身确实是测不出来的 —— 它不该被当作验收证据。**

### 4.3 必须一并声明的**作用面边界**（勿过度宣称）

1. 确定性判据只覆盖 **`ALTER_PATCHES` 的 3 张表 × 稳态 schema**。
   `BACKFILL_PATCH`（`pg.rs:437`）的 UPDATE **仍无条件执行**，仍取 `RowExclusiveLock`。
   REQ-039 **没有**、也**不声称**消除这一把锁。
2. 空库 / 旧库路径上，那三条 ALTER **必然要发**（列确实不存在），排他锁照取。
   REQ-039 消除的是「稳态库上重复取排他锁」，不是「所有 ALTER 的排他锁」。
3. 「零死锁」是**结构论证 + 确定性判据**的结论；`40P01` 仍是共享集群上的环境类
   失败模式之一，报任何数字都必须带集群状态（见 §5）。

---

## 5. Q5 · 全量测试与 AC-005

### 5.1 全量

```
cargo test --offline --no-fail-fast
→ exit=101  elapsed=304s
→ 33 个测试二进制的 "test result:" 行求和
  passed=522  failed=2  ignored=1
```

AC-005 目标 `521 passed / 0 failed / 1 ignored`。**总数不劣化**（522 ≥ 521），
耗时 **304s ≈ 5.1 分钟**，落在「基线约 4~5 分钟」区间，**未劣化到 10 分钟**。

### 5.2 那 2 个失败：**确为环境类**，且**代码失败数为 0**

| 失败测试 | 二进制 | 报错 |
| --- | --- | --- |
| `test_empty_concept_group_collection_roundtrips_as_empty_not_null` | `test_concept_store` | `tests/common/mod.rs:66` `Connect(Io(Os { code: 104, kind: ConnectionReset }))` |
| `test_snapshot_placeholder_structures_present_when_empty` | `test_snapshot` | 同上，逐字相同 |

**逐条核实 Dev 的说法（全部成立）：**

1. **发生在建连接时，不在迁移之后** —— 属实。错误变体是 `sqlx::Error::Connect(Io(..))`，
   即 `PgPoolOptions::connect()` 的错误。而 `tests/common/mod.rs:60-66`：

   ```rust
   pub async fn test_pool_with(max_connections: u32) -> PgPool {
       let url = ontok::config::test_database_url().expect(...);
       pool::connect_for_tests(&url, max_connections)
           .await
           .expect("连不上测试库")     // ← 第 66 行
   }
   ```

   这行在**任何 `migrate()` 被调用之前**执行 ⇒ **迁移代码根本没跑到**，
   与 REQ-039 的改动无因果关系。
2. **单跑通过** —— 属实：

   ```
   cargo test --offline --test test_concept_store -- --test-threads=1
   → ok. 5 passed; 0 failed; ... finished in 33.27s
   cargo test --offline --test test_snapshot -- --test-threads=1
   → ok. 10 passed; 0 failed; ... finished in 61.44s
   ```
3. **共享集群连接数** —— 测量时刻：**集群总连接 37 / `max_connections` 100**，
   本库 `ontol` 1 条，`SELECT 1` **160 ms**。属 AC「环境事实」的慢集群区间（57~170 ms）。

### 5.3 代码失败 vs 环境类失败（分开写）

| 类别 | 次数 | 判定依据 |
| --- | --- | --- |
| **代码失败**（REQ-039 引入或暴露的真缺陷） | **0** | 522 条里无一条因 REQ-039 逻辑失败；2 个失败均为 `Connect(Io)` 且单跑通过 |
| **环境类** `Connection reset by peer` | 2 | 建连接阶段（迁移前），单跑通过 |
| **环境类** `40P01` | **0** | AC-001 10 轮 + K=3 30 次运行，出现 0 次 |
| **遗留缺陷** REQ-040 前提竞态 | K=3 下 7/30 | 已登记为 REQ-040，**非 REQ-039 引入**（见 §8.1） |

**⇒ 全量无「代码失败」。AC-005 通过。**

---

## 6. Q6 · 红线未被破坏 —— **全部守住**

| 红线 | 核验方式 | 结果 |
| --- | --- | --- |
| `tests/drop_rate.rs` 零改动 | sha256 HEAD vs develop | `5da9272b34aab0a55d66b90d8055ad4cec434ee1256878a461ba91384d4d4363` **逐字节相同**；diff 0 行 |
| `tests/test_fk_naming.rs` 零改动 | sha256 HEAD vs develop | `c28a5bb65e8d796266a0dcfb6340a6b1cab44060cb4d3b302fcd288e0ef82831` **逐字节相同**；diff 0 行 |
| `src/domain/view.rs` 零改动 | `git diff develop ab2f56c` | **0 行** |
| `test_humanize_still_refuses_unknown_columns` 未放宽 | 读 `src/domain/view.rs:1254` | 4 条 `assert_eq!(humanize_column(bad), None, ...)` **一条不少、一条未改** |
| 没有测试被删除 | 计数对比 | 测试数 **491（develop）→ 494（HEAD）**，只增不减 |
| 没有测试被 `#[ignore]` | `git grep` 对比 | HEAD 与 develop **各 2 处**且**位置逐字相同**（`tests/test_mcp_server.rs:550/553`），**全部为既有**，新增 0 处 |
| `test_backfill_…` 未被改 | `git diff develop ab2f56c -- tests/test_snapshot_write_paths.rs` | **0 行** |

**⇒ AC-002 / AC-006 通过。修复不是靠关测试做成的。**

---

## 7. Q7 · 产物未变 —— **通过，sha256 精确匹配**

```bash
cd /home/myg133/Projects/Anchnet/Ontology/code   # 必须在 code/ 下跑，否则读不到 src/extract/table_names.json
./target/debug/ontok-propose \
  --schema /home/myg133/Projects/Anchnet/Ontology/code/out/e2e-schema.json \
  --out   <ws>/artifacts/proposal.json \
  --stats <ws>/artifacts/stats.json \
  --project extracted
# exit=0
```

| 产物 | 基线 sha256 | 实测 sha256 | 结论 |
| --- | --- | --- | --- |
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | **一致** |
| `proposal.json`（`--project extracted`） | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | **一致** |

（第一次在仓库根跑失败，报「读不到翻译表 src/extract/table_names.json」—— 相对路径问题，
切到 `code/` 后正常，非缺陷。）

---

## 8. Q8 · 两个交付给负责人的决策项

### 8.1 REQ-040（backfill 前提竞态）该不该现在就修

**建议现在就修，但走 REQ-040 独立立项，不要塞进 REQ-039。**

**我实测到它在新代码上是活的**，不是纸面风险：

- K=3 跨进程下，`test_backfill_is_deterministic_preserves_and_idempotent`
  在**修复体失败 2/15**，在 **M1 缺陷体失败 5/15**；
- 30 次 K=3 运行的**全部 7 个失败都是它**，签名一致：
  `tests/test_snapshot_write_paths.rs:680` `left: [0,1,2]` / `[0,0,1]` vs `right: [0,0,0]`。

**支持立刻修：**

1. **它在持续污染验收信号。** REQ-039 立项正是因为 REQ-038 验收带出了同样的
   「全量随机红」。只要它在，后续每条需求的验收都要先花一轮区分「它红了是不是自己
   造成的」——这正是 REQ-039 走过的弯路，不该让 REQ-041+ 重走。
2. **修法纯测试侧、极小、不放松任何判据。** registry 已写明两种：① 3 条 INSERT 合成
   一条多行 INSERT，或 ② 插入与前提断言包进同一事务。两种都**不碰**被测性质
   （确定性/保序/幂等），因此**不违反 AC-002 的实质**（AC-002 禁的是「删掉报错的测试」，
   不是「修掉测试的竞态前提」）。
3. **成本远低于收益**：约 5 行改动，换掉一个让全量结果不可判读的噪声源。

**但必须独立立项，理由是纪律：**

- AC-002 明文点名 `test_backfill_is_deterministic_preserves_and_idempotent`
  「**不能动它**」。在 REQ-039 里动它会让 REQ-039 的验收自相矛盾；
- `BA/dispatch/req-registry.md:90` 里 REQ-040 已是「**已就绪**」，已写清根因与两种修法
  （并记录「生产侧无锁可解」）。**BA 的判断与我的独立实测一致。**

⇒ **决策建议：立刻派单做 REQ-040，状态「已就绪」→「进行中」。REQ-039 本轮按现状通过。**

**一处需负责人注意的口径修正：** Dev 自述「baseline 9/15、fixed 1/15」字面上像
**修复后更差**；我测得 fixed 13/15、M1 10/15，方向相反但都红。这三个数字都是
**同一竞态在不同负载下的采样**，互相不可比。建议 REQ-040 的验收标准里
**不要写死这个比率**——它是竞态，不是稳定缺陷率。

### 8.2 `to_regclass()` 依赖 `search_path`，可能解析到 `pg_temp` ⇒ **我认同 Dev：不改**

Dev 的理由是「保守方向，只多发 DDL，不会漏发」。我**实测复现了这个机制**：

```
QA_TEMP to_regclass_after_temp_create  = "action_types"
QA_TEMP ordinal_seen_while_temp_exists = false      ← 遮蔽确实发生，且答「列不存在」
QA_TEMP ordinal_seen_after_temp_drop   = true
QA_TEMP search_path = "$user", public
```

⇒ **机制成立**，且**失败方向确实保守**：答成「列不存在」⇒ 多发一条
`ADD COLUMN IF NOT EXISTS` ⇒ 只是白开一次排他锁，**绝不会漏建列**。

**我额外查了触发面，结论是「不改」更站得住：**

- `src/` 下 `grep 'CREATE TEMP|search_path|temp_schema|pg_temp'` ⇒ **0 命中**。
  生产连接池（`src/db/pool.rs:26-31`）**从不设置 `search_path`**；
- `search_path` 只在**测试辅助函数**里被改（`tests/test_db.rs:149`、`:315`），
  而那些函数都强制 `max_connections(1)`（`:306` 注释写明是硬要求），
  **每池一条连接、连接级设置不会串**；
- 触发它需要有人在跑 `migrate()` 的同一会话里建一张**恰好同名**的临时表，
  仓库里没有这样的代码。

**⇒ 概率低 + 方向保守（最坏只是退化成 REQ-039 修复前的行为，即「多发 DDL」），
不改是正确的取舍。我认同 Dev。**

**唯一建议（非阻断，可选）：** 在 `patched_columns_present` 注释里补一句
「`to_regclass` 受 `search_path` 影响；若被同名 `pg_temp` 表遮蔽，失败方向是**多发**
而非漏发」，把边界写进代码而不是只留在对话里。

---

## 9. 发现但未修的问题（如实登记）

| # | 问题 | 严重度 | 归属 |
| --- | --- | --- | --- |
| 1 | REQ-040：`test_backfill_…` 前提竞态，K=3 下 7/30 红（修复体 2/15、M1 5/15），签名 `left:[0,1,2] right:[0,0,0]` | **中**（污染验收信号） | **REQ-040**，已「已就绪」，建议立即派单 |
| 2 | Dev 把 M2 称作「等价变异」不准确：索引错位时该断言是**唯一**能发现问题的守卫，删掉后 `migrate()` 静默 `Ok` 并给出错位答案 | **低**（有别的判据兜底） | 建议改注释措辞，断言**保留** |
| 3 | `to_regclass` 受 `search_path` 影响（已实测复现），失败方向保守 | **低**（生产路径无触发代码） | 认同不改；建议补注释 |
| 4 | `BACKFILL_PATCH` 的 UPDATE 仍无条件执行、仍取 `RowExclusiveLock`；REQ-039 未处理也未声称处理 | **信息** | 记入 §4.3 作用面边界 |

**没有发现 Dev 这轮「修了但没修好」的地方。** 解码、形状守卫、判据覆盖面三项都真的修好了。

---

## 10. 我自己引入又清掉的问题（如实登记）

| # | 我做的事 | 处置 | 证据 |
| --- | --- | --- | --- |
| 1 | 在 `src/db/pg.rs` 尾部**临时追加** `qa_probe` / `qa_probe2` / `qa_cluster` / `qa_temp` 四个 `#[cfg(test)]` 探针模块 | 全部删除，从 `ab2f56c` 原版副本复原 | 复原后 `sha256(src/db/pg.rs)=0df06400988d9ee691eb2c6313a91e486ed1adc80310719627b014271f644f30`，与 `git show ab2f56c:src/db/pg.rs` **一致** |
| 2 | 9 次临时变异改写（`add4th`/`m1`/`decode_only`/`consumer_only`/`m2_nocolcheck`/`sel_drops_col`/`sel_dupes_col`/`sel_prepends_col`/`sel_has_from`） | 每次跑完立即复原 | 同上；末态 `git status --porcelain` = **0 行** |
| 3 | `ontok-propose` 第一次在仓库根跑失败（「读不到翻译表」） | 切到 `code/` 重跑成功 | 相对路径问题，非缺陷 |
| 4 | 一处 `sel.pop()` 变异因 `sel` 未声明 `mut` **编译失败** | 修正变异脚本后重跑 | **如实计入：编译失败不计为「变异执行」**；修正后每个变异都有独立运行条数（§3.4） |
| 5 | **纪律偏离**：本环境 `write`/`edit` 工具对 `QA-REPORT.md` 连续 4 次失败（含最小内容），故本报告用 **quoted heredoc**（`<<'ENDOFREPORT'`）落盘而非 write 工具 | 已如实登记 | 反引号经 quoted heredoc 原样保留，未被 shell 展开 |

**纪律遵守自查：**

- **未使用** `CARGO_TARGET_DIR`、**未使用** `--release`、**未另建 probe crate**
  （探针一律以 `#[cfg(test)] mod` 挂在既有 crate 内，复用热 target）；
- Dev 留下的 `target` 符号链接**全程保持原样**（复查仍指向 `code/target`）；
- 口令只经环境变量 `ONTOK_TEST_DATABASE_URL` 传递，**未写入任何文件/日志/commit**；
  本报告不含口令（连接串只显示 `host:port/db`）；
- 共享本体库**未执行任何 DELETE / TRUNCATE**；临时表仅用**会话私有 `pg_temp`** 且用完 `DROP`；
- **未 push**（仓库无 remote）、未 squash、未 force、未改历史；
- 临时产物目录：`.review-workspace/verify-req039b/`；
- **`.review-workspace/fix-props/schema.json` 全程未碰**（复查时间戳仍为 10月 7日 23:55）。

---

## 11. 交付状态

**最终态：worktree 干净，等于 `ab2f56c`。**

```
$ git rev-parse HEAD
ab2f56cc9b2869e3e3b1a5ba5a045f22105b67a2
$ git status --porcelain | wc -l
0
$ sha256sum src/db/pg.rs       # 与 git show ab2f56c:src/db/pg.rs 一致
0df06400988d9ee691eb2c6313a91e486ed1adc80310719627b014271f644f30
$ sha256sum tests/test_db.rs   # 与 git show ab2f56c:tests/test_db.rs 一致
ed5353af2c8f074f6ba1c04816bf7faccfd722f5a4b99c18c9d569d66703f3b8
$ ls -la target
lrwxrwxrwx ... target -> /home/myg133/Projects/Anchnet/Ontology/code/target
```

复原后**最终确认跑**：`cargo test --offline --test test_db -- --test-threads=1`
⇒ `ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 38.13s`

中间日志（`*.log`/`*.rc`/变异脚本/产物副本）保留供复核，报告本身 `QA-REPORT.md` 保留。

---

## 12. 逐条 AC 判定

| AC | 判定 | 依据 |
| --- | --- | --- |
| AC-001 死锁不再复现 | **通过（附口径警告）** | 顺序 10 轮 10/10 绿、0 × 40P01。但 §4.1 证明该口径在缺陷体上也是 0，**零判别力**，不应单独作证据 |
| AC-002 修复不是靠关测试 | **通过** | 测试数 491→494；`#[ignore]` 无新增；`test_backfill_…` 0 行改动 |
| AC-003 空 schema 不漏建 | **通过** | `test_migrate_on_empty_schema_builds_every_table_and_column` + `…_survives_concurrent_first_run` 均在 11/11 内通过 |
| AC-004 变异自证 | **通过** | 4 条变异全部真跑，杀死者给到二进制名 + 运行条数；2 条存活逐条论证 |
| AC-005 全量不劣化 | **通过** | 522 passed / 2 failed / 1 ignored，304s；2 个失败为环境类，单跑通过，**代码失败 0** |
| AC-006 红线 | **通过** | 3 文件 sha256 逐字节相同；断言未放宽 |
| AC-007 产物未变 | **通过** | 两个 sha256 精确匹配 |

---

## 13. VERDICT

**PASS 带非阻断。**

- REQ-039 修复轮**真的修好了上一轮查出的缺陷**（Q1 有对照组佐证，Q2 三个方向都验证到报错）；
- 判据**获得了判别力**（Q3：M1 被杀死且红在正确的表上；并补做了 Dev 未做的两条单边变异）；
- 红线**全部守住**，产物**未变**，全量**无代码失败**；
- 唯一实质异议是 Dev 对 M2 的「等价变异」定性不准（§3.3）——**属表述问题，不影响判据结论，不阻断**；
- 唯一的真实遗留缺陷是 REQ-040 的前提竞态，**已实测其在新代码上复现**，建议立即以 REQ-040 立项修复。
