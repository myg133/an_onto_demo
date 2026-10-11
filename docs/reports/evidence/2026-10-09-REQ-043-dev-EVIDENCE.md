# REQ-043 Dev 证据汇总

> 口径：所有变异均在 worktree `/home/myg133/Projects/Anchnet/Ontology/feature-REQ-043`、
> 分支 `feature/REQ-043`（基线 `14e0f1e`）上**实际执行**。
> 命令一律为 `cargo test --test test_connect_retry`（二进制 `test_connect_retry`，共 12 条）。
> **编译失败不计为变异执行**（本轮发生过 1 次，见 §4）。
> 临时文件全在 `.review-workspace/req043/`；未用 `/tmp`、未用 home。

## 1. 修复前基线复现（工作树 = `14e0f1e`，未改任何判据）

| 变异 | 上轮 QA | 本轮 Dev 实测 | 判定 |
| --- | --- | --- | --- |
| BA | 存活 | `test_connect_retry` **12P/0F** | 一致 |
| BB | 存活 | **12P/0F** | 一致 |
| BC | 假红 | **10P/2F**（`test_retry_scope_is_connect_only` + `test_every_test_side_connect_site_goes_through_retry` 双 panic） | 一致 |
| C3 | 存活 | **12P/0F** | 一致 |
| C3b | 存活 | **12P/0F** | 一致 |
| R3b | 存活 | **12P/0F** | 一致 |

BC 的 panic 原文（证明是**提取器逃逸**而非真红）：

```
panicked at tests/test_connect_retry.rs:293:5:
从第 560 个字节起的括号没有配平 —— 判据读到的是残缺源码
```

## 2. AC-005 变异表（修复后）

| 变异 | 二进制 | 运行条数 | 结果 | 杀死它的判据（断言位置） |
| --- | --- | --- | --- | --- |
| **BA**（行尾注释裸 `)`） | `test_connect_retry` | 11 passed / **1 failed**（共 12） | 红 | 第 ① 条禁用词断言 `test_connect_retry.rs:489` |
| **BB**（块注释裸 `)`） | `test_connect_retry` | 11P / **1F** | 红 | 同上 `:489` |
| **BC**（行尾注释裸 `(`） | `test_connect_retry` | 11P / **1F** | 红（**真红**） | 同上 `:489`；**不再**是 `:293` 的 panic 逃逸 |
| **C3**（次数 `0..=2` → `0..=0`） | `test_connect_retry` | 11P / **1F** | 红 | `test_lib_unit_test_connect_also_retries` `:787`「只推导出 1 次尝试」 |
| **C3b**（删退避 + `0..=1`） | `test_connect_retry` | 11P / **1F** | 红 | 同上 `:799`「循环体里没有退避（找不到 sleep）」 |
| **R3b**（helper 名 `qa_hook`） | `test_connect_retry` | 11P / **1F** | 红 | 第 ③ 条白名单制 `:511`「解析不到干净函数体」 |

6 条全部 `EXECUTED`（无一条 BUILD_FAIL），**0 存活**。

## 3. 补充变异（证明「为什么堵得住」，非 AC-005 要求）

| 变异 | 二进制 | 运行条数 | 结果 | 说明 |
| --- | --- | --- | --- | --- |
| **BD** 嵌套块注释 `/* 外 /* ) */ 仍在 */` | `test_connect_retry` | 11P/1F | 红 `:489` | 证明 `/* */` 按**深度计数**，而非「首个 `*/` 即止」 |
| **BE2** 生命周期 `'static` 置于 `migrate` 之前 | `test_connect_retry` | 11P/1F | 红 `:489` | 证明 `'` 未被误判成字符字面量（若误判会吞掉 `migrate` ⇒ 假绿） |
| **BF** 字符字面量 `')'` | `test_connect_retry` | 11P/1F | 红 `:489` | 证明字面量被跳过、且不被 FORBIDDEN 误判 |
| **BG** 良性注释 `// ) ( /* )` 且**不含** migrate | `test_connect_retry` | **12P/0F** | **绿**（预期绿） | 假红回归控制轮 |
| **C4**（F-4 未做） | `test_connect_retry` | 12P/0F | **存活** | 如实复核，见 §5 |

## 4. 自引入又清掉的问题（登记）

1. **BD 首轮 BUILD_FAIL（不计执行）**：`supp.py` 的 `body()` helper 返回了含 `%s` 的字符串却**漏了 `%` 运算符**，
   于是字面量 `%s` 被写进 `tests/common/mod.rs`，编译报 ``expected expression, found `%` ``。
   按纪律**编译失败不算变异执行**，已修 helper 后重跑 BD，得到上表的 11P/1F。
2. **白名单初版为空 ⇒ 引入假红**：`BG` 控制轮实测把合法闭包 `|| async { … Ok(p) }` 判红
   （`Ok` 解析不到 `pool.rs` 干净体）。已把 Rust 预置构造器 `Ok` / `Err` / `Some` / `None`
   补进 `CALLED_IDENTS_ALLOWED` 并写明理由；复跑 BG 回到 12P/0F。
   **这条是本轮自己引入又清掉的，如实登记。**

## 5. 存活变异逐条定性

| 存活项 | 定性 | 论证 |
| --- | --- | --- |
| **C4** | **判据有洞（已知，本需求范围外）** | `connect_sites` 仍按 `before.ends_with("_")` 跳过 `qa_db_connect(`。F-4 未列入本需求（briefing 只要求 F-6 / F-7 两条低成本订正），故**如实说明它仍存活**。要杀它须把豁免收窄为「仅 `retry_connect(` / `after_connect(`」。 |
| **C5**（新开测试文件） | **判据有洞（已知，本需求明确不做）** | 覆盖判据仍硬编码枚举 2 个文件。F-5 成本高，briefing 明确**本需求不做**。 |

## 6. F-5 局限（如实说明）

`test_every_test_side_connect_site_goes_through_retry` 仍只枚举
`tests/test_db.rs` + `tests/common/mod.rs` 两个 `include_str!` 字面量
（`include_str!` 只接受字面量，这是该做法的结构性约束）。
**任何新开的测试文件都不在覆盖内** —— 在新文件里写一处裸
`connect_for_tests(...)` 不套重试，本判据看不见（QA 的 C5 实测 12P/0F）。
因此 AC-004 的覆盖面**不是全仓完备性证明**，只是一道针对既有两文件的回归护栏。
本需求**未**改这一点（briefing 明确列为「不要求做」）。

## 7. 全量与产物

### 7.1 全量 `cargo test --no-fail-fast`

| 轮次 | 结果 | 墙钟（`time` 实测） |
| --- | --- | --- |
| 第 1 轮 | 36 二进制 / **532 passed / 4 failed / 1 ignored** | 6m32.665s |
| 第 2 轮（定稿） | 36 二进制 / **536 passed / 0 failed / 1 ignored** | **6m12.031s** |

环境口径：`ONTOK_TEST_DATABASE_URL` 由仓库根 `.env` 在运行时取连接串并把 dbname
改写为 `ontol`（`.env` 里库名实为 `notol`）。**全程未 echo 连接串**；口令只经环境变量。

**第 1 轮那 4 条失败 = 环境类，不是代码**（运行期判据与环境类失败严格二分）：

| 失败项 | 二进制 | 失败原因 |
| --- | --- | --- |
| `test_version_seq_increments_and_is_isolated` | `test_project`（**本需求零改动**） | `Db(Io(Os { code: 104, kind: ConnectionReset }))` |
| `test_version_seq_under_concurrency_is_unique` | 同上 | 同上 |
| `test_version_frozen_at_is_none_before_freeze` | 同上 | 同上 |
| `test_version_freeze_sets_frozen_at_and_is_idempotent` | 同上 | 同上 |

四条全部 panic 在 `tests/test_project.rs:11:23`（`store()` 里的 `s.migrate().await.unwrap()`），
即**真实建连**阶段；`test_project.rs` 不在本需求 diff 内
（`git diff --name-only 14e0f1e HEAD` 只有 `tests/test_connect_retry.rs` 与 `tests/test_db.rs`）。
**单独复跑 `cargo test --test test_project` => 10 passed / 0 failed（21.02s）**，确认为偶发环境失败。

### 7.2 产物 sha（AC-007）

```
cargo run --bin ontok-propose -- \
  --schema /home/myg133/Projects/Anchnet/Ontology/code/out/e2e-schema.json \
  --names src/extract/table_names.json \
  --out   .review-workspace/req043/proposal.json \
  --stats .review-workspace/req043/stats.json \
  --project extracted
```

| 产物 | 期望 sha256 | 实测 sha256 | 结论 |
| --- | --- | --- | --- |
| `stats.json` | `cda6a9a2…7632cd7ba` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | **逐字节吻合** |
| `proposal.json` | `8dbd32db…b1db3e4c3e` | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | **逐字节吻合** |

## 8. 红线自查

| 红线 | 实测 |
| --- | --- |
| `tests/drop_rate.rs` 零改动 | 未出现在 `git diff --name-only 14e0f1e HEAD` |
| `tests/test_fk_naming.rs` 零改动 | 同上 |
| `src/domain/view.rs` 零改动 | 同上（本需求**未改任何 `src/` 生产代码**） |
| 无测试被删除 / `#[ignore]` | `git diff 14e0f1e HEAD` 中 `#[test]` / `#[tokio::test]` / `#[ignore]` **零增删**；函数名集合差异仅 3 个**新增 helper**（`attempts_from_for_header` / `is_char_literal` / `retry_loop`） |
| `test_humanize_still_refuses_unknown_columns` 不放宽 | 所在文件未改动 |
| `test_backfill_…` 不放松 | 所在文件未改动 |
| 不改重试行为参数 | `tests/common/mod.rs` / `src/db/apply.rs` / `src/db/pool.rs` **全部零改动**（只读未写） |
| `tests/test_db.rs` 只改注释 | 该文件 diff 的**非注释增删行为 0 行** |
| 不设 `CARGO_TARGET_DIR` / `--release` / probe crate | 全程未设 |
| 无 remote、不 push | 未执行任何 push |
| 临时文件位置 | 全在 `.review-workspace/req043/`；未用 `/tmp`、未用 home |
| `feature-REQ-043/target` 符号链接 | 保持原样（仍指向 `code/target`） |

## 9. 与 QA 报告的一处事实订正（发现，未改代码）

QA 报告称 R3b 之所以存活是「实参里 `connect` 字样只出现在行内注释」。
**实测不是**：反断言 `arg.contains("connect")` 之所以通过，是因为实参里
`max_connections` **本身就含子串 `connect`**（`max_` + `connections`）。
而 `normalize` 会剥掉整行的 `//` 注释 —— 那行注释在判据里根本不存在。
结论（存活 ⇒ 判据有洞、不是等价变异）不变，但**成因描述有误**，如实登记。