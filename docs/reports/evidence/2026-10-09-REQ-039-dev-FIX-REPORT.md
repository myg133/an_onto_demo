# REQ-039 修复轮 · 证据索引（2026-10-09）

所有量化数字都标注了**口径 + 命令**。共享集群状态：db=`ontol`，PG 16.14，
`max_connections=100`，测量期间 backends 在 **37~38** 之间浮动。

延迟基线（`SELECT 1`，n=15，同一连接池）：
- min=155ms **p50=156ms** max=196ms

---

## ① 解码缺陷复现（修复前）

二进制 `zz_tmp_req039_probe`（临时探针，测完已删），`--test-threads=1`：

```
DECODE Vec<(bool,)>          -> len=1 values=[true]
RAW rows=1 cols_of_row0=3
DECODE by column index     -> [true, true, true]
```

口径：`sqlx::query_as::<_, Vec<(bool,)>>` 对一行三列只取第 0 列、不校验列数。

---

## ② 判别力自证

命令：`cargo test --offline --test test_db -- --test-threads=1`

| 版本 | test_db 结果 | 说明 |
| --- | --- | --- |
| 修复后 | **11 passed / 0 failed**，38.28s | — |
| 变异体 M1（有缺陷版） | **10 passed / 1 FAILED**，58.80s | 红在 `table=action_types`，`55P03 lock timeout` |

M1 = 退回 `Vec<(bool,)>` 解码 **且** 退回 `present.get(i).copied().unwrap_or(false)`。
两者都退回才是**忠实复现已交付版本**；只退解码、用 `zip` 消费会是另一个缺陷
（`zip` 静默跳过 ⇒ 压根不发 ALTER），那测的是别的东西。

失败定位精确：`ontology_versions`（索引 0）通过，`action_types`（索引 1）红 ——
与「只有第 0 列被正确解码」完全吻合。

### 变异体 M2：去掉列数形状断言

结果：**11 passed / 0 failed**（存活）。
判定：**等价变异**。`try_get(i)` 循环本身已正确解码全部 N 列，
该断言只防将来有人改坏 `sel` 生成方式，属纵深防御，不是当前行为的一部分。

---

## ③ K=3 压力（`stress2.sh 3 5 <tag>`）

口径：3 进程并发 × 5 轮 = 15 次「进程运行」，每进程跑
`cargo test --test test_snapshot_write_paths -- --test-threads=16`（8 个测试）。

| 版本 | pass | fail | 40P01 | codefail |
| --- | --- | --- | --- | --- |
| baseline（= 上一轮已交付） | 6/15 | 9 | **0** | 9 |
| 修复后 | 14/15 | 1 | **0** | 1 |

⚠️ **两侧 40P01 都是 0** ⇒ **K=3 这个口径区分不出修复前后**。
baseline 出现 0 死锁是因为 `ontology_versions` 恰是第 0 条 ALTER、短路命中即断环
（QA 的判断，此处独立复现成立）。真正的判别证据是 ② 的确定性判据，不是这个压力数。
codefail 全部是 REQ-040 那条 `test_backfill_is_deterministic_preserves_and_idempotent`
（跨进程互相污染，见下）。

---

## ④ 顺序跑 10 轮（`rounds2.sh 10 rounds-fixed-k1`）

口径：单进程顺序 10 次同一二进制，8 个测试并行。

```
round=01..10 全部 PASS
SUMMARY tag=rounds-fixed-k1 total=10 pass=10 fail=0 | env40P01=0 envconn=0 codefail=0
```

逐轮耗时：23.4 / 13.9 / 14.7 / 17.9 / 15.7 / 16.2 / 15.3 / 19.6 / 20.2 / 15.3 s
逐轮 cluster：37 / 37 / 38 / 37 / 37 / 38 / 37 / 37 / 37 / 38 backends

---

## ⑤ 全量 `cargo test --offline --no-fail-fast`

| 运行 | 耗时 | passed | failed | ignored | 失败项 |
| --- | --- | --- | --- | --- | --- |
| fixed-full-1 | **312s** | 523 | 1 | 1 | `test_migrate_on_empty_schema_survives_concurrent_first_run` |
| fixed-full-2 | **295s** | 523 | 1 | 1 | `raw_identifiers_never_reach_the_output_structurally` |

两次都是 **35 个测试二进制**，`rc=101`。

**两次失败是同一类、且与本改动无关**：
都是 `ConnectionReset / Connection reset by peer`，且都发生在
`tests/common/mod.rs:66` 的 **建连接**阶段（迁移尚未开始）：

```
连不上测试库: Connect(Io(Os { code: 104, kind: ConnectionReset, ... }))
```

两次失败的是**不同**测试 ⇒ 随机落在连接数高的测试上，是共享集群连接槽位 flake。
单独重跑各自绿：
- `--test test_db` → **11 passed**，17.67s
- `--test test_no_write_path` → **4 passed**，0.54s

⚠️ 全量跑时 35 个二进制并发建池，把 backends 顶到 37~38（上限 100，
另有 5 个业务库常年占约 30 条）。这是**已知的集群环境限制**，
不是 REQ-039 引入的；本仓 `src/db/pool.rs` 与 `tests/common/mod.rs` 的注释里
已记录同一现象（"Connection reset by peer"）。

---

## ⑥ 未修问题（如实记录）

1. **REQ-040**：`test_backfill_is_deterministic_preserves_and_idempotent`
   在**跨进程**并发下必然红（断言「迁移前历史行全是 DEFAULT 0」，
   但并发的另一个进程可能已经跑过回填）。baseline 9/15 红、修复后 1/15 红，
   两侧都红 ⇒ 与本改动无关。红线明确「不许改」，留给 REQ-040。
2. **全量跑连接数**：35 个测试二进制并发建池会撞共享集群连接上限。
   本轮如实报告为 flake，未改连接数策略（越权）。
3. **`patched_columns_present` 的 `to_regclass()` 依赖 `search_path`**：
   多连接池下若某连接的 `search_path` 被改（如本文件那条 `pg_temp` 测试），
   探针可能落到 `pg_temp` 而判「列不存在」⇒ 多发 ALTER。
   这是**保守方向**（多发而非少发），不造成漏建，故本轮未动。

---

## ⑦ 自己引入又清掉的问题（如实记录）

1. 第一次注入 M1 时，在消费端留下了未删除的 `if !present { ... }` 死分支，
   报 `E0600: cannot apply unary operator ! to type Vec<bool>`，**编译失败**。
   那一次**不算变异运行**（根本没跑起来）。删掉死分支后重跑，才有上面
   「10 passed / 1 FAILED」这个真结果。
2. 补文档时新加的小节与上一节之间漏了空 `///`，两段被 rustdoc 合成一段
   （`# 单次往返` 与新标题粘连）。已补 `///`。
3. 早期误以为 `zip` + 短 `Vec` 会复现同一缺陷 —— 不会：`zip` 会**跳过**而非
   发 ALTER，那是另一个（相反方向的）缺陷。M1 因此必须连消费端一起退回，
   才忠实。已在上文口径里写明。