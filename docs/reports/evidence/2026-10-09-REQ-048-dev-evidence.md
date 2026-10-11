# REQ-048 · 执行证据台账

> 供 BA 复核。所有数字均标口径与命令。二进制：`feature-REQ-048/target/debug/ontok-gate-preflight`（debug，未用 `--release`，未用 `CARGO_TARGET_DIR`，未 `ln -s`）。

## 全量回归

- 命令：`ONTOK_TEST_DATABASE_URL=<取自 .env，库名换 ontol> cargo test --no-fail-fast`
- 结果：**539 passed / 0 failed / 1 ignored**，测试二进制 38 个，`EXIT=0`，耗时 **419 秒**
- 口径：`awk` 逐行解析 `req048/fulltest.log` 中 38 行 `test result:` 的 `passed;/failed;/ignored;` 字段求和（**未用** `paste -sd+`）
- 环境类错误（`55P03` / `Connection reset by peer`）：`grep -ciE` 命中 **0** 次
- 原始日志：`req048/fulltest.log`

## 四种情形（各一条判据，均为真实二进制执行）

| 情形 | 判据 | 实跑 | 退出码 |
| --- | --- | --- | --- |
| 1a 正常（默认路径校验现有产物） | 读回真实 91 表产物并全判据通过 | 2 次 | 0 |
| 1b 正常（真实拉库全链路） | 拉取→校验→原子落盘→读回复验→打印成功 | 1 次 | 0 |
| 2 源库不可达 | 非 0 + 指引可读 + 口令未回显 | 1 次 | 1 |
| 3 产物非合法 JSON | 非 0 + 报出解析位置 | 1 次 | 1 |
| 4a 表数异常偏少（3 表） | 非 0 + 报出低于下界 | 1 次 | 1 |
| 4b 空壳（0 表） | 非 0 | 1 次 | 1 |
| 4c **表头撒谎**（自称 91/1051，tables 只有 3 张） | 非 0 + 三条一致性报错 | 1 次 | 1 |
| 附：两门禁缺省路径解析不一致 | 拒绝猜，要求设 `ONTOK_SCHEMA_JSON` | 2 次 | 1 |

- **未跑**：情形 1「默认阈值 + 真实源库」。源库连接串本会话不存在（见下）。
  1b 用的是**测试库** `ontol`（11 表 / 85 列），因此阈值下调为 `--min-tables 5 --min-columns 50`。

## 变异实验（MUT-A）

- 变异内容：`validate()` 整体短路，直接 `return Vec::new()`（**编译通过**后才执行，不拿编译失败冒充）
- 执行：4 个坏产物 + 1 次真实拉库 = 5 次
- 结果：`scenario4-*` 三个产物全部由 EXIT=1 翻成 **EXIT=0**；真实拉测试库也被放行并落盘。
  仅 `scenario3` 仍失败（它的判据在 `serde_json` 反序列化，位于 `validate()` 之前，与本次变异无关）。
- 论证：判据**不是摆设**，且四类情形各有**不同**的拦截点，非同一条判据一锅端。
- 撤销：从 `req048/preflight.pristine.rs` 恢复，`sha256 = c4173cfee152a9521e592ebbd083b40d7fc3effacb78541d1074cea594ce4c8f`（与变异前一致），重建后 4 个坏产物重新全部 EXIT=1、默认路径 EXIT=0。

## 关键发现（未修，见报告）

1. `.env` 里的库名 `notol` 在本机实例上**不存在**（服务端回 `database "notol" does not exist`）；
   实例上实际存在的是测试库 `ontol`（11 表 / 85 列）。
2. **源库连接串本会话不可得**：`.env` 只有 `ONTOK_TEST_DATABASE_URL`；
   `.env.example` 与 README 里源库地址均为占位/同一失效库名。
   ⇒ 默认阈值下的「正常路径」无法端到端实跑，如实登记为**未跑**。
3. 本命令**拦住过一次真实的「拿测试库当源库拉」**：11 表 < 下界 80 ⇒ 拒绝落盘，
   门禁产物 `sha256 b71c2afd…` 前后一致，**没被覆盖**。

## 产物完整性

- 门禁输入 `.review-workspace/fix-props/schema.json`：**未被本需求改动**
  （sha256 `b71c2afd…`，与拉取前备份逐字节一致）
- 两个门禁测试在全量回归中均 `ok`
- worktree 干净，本次仅 1 个提交 `171d2a0`，`git diff d521f79 HEAD` 只有 1 个新增文件、0 处修改