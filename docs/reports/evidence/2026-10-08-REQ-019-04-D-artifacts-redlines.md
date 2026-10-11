# D · 红线与产物

## D-1 红线文件：全部未动 ✅
`git diff --name-only c8385f5..1c6914e`（本轮全部改动，仅 4 个文件）：
```
src/agent/mod.rs
src/agent/orchestrate.rs
src/bin/ontok-propose.rs
tests/test_extract_cli_req019.rs
```
显式检查红线（`git diff c8385f5..1c6914e -- <file>` 输出为空）：
- tests/drop_rate.rs —— 未动 ✅
- tests/test_fk_naming.rs —— 未动 ✅
- src/domain/view.rs —— 未动 ✅
- .review-workspace/fix-props/schema.json —— **存在，未被删**
  （219688 字节，时间戳 10月 7日 23:55，早于本轮 10月 8日 02:55）✅

## D-2 产物 sha：完全对上 ✅
命令（在 worktree 自带 target 上跑，round 1 = 默认）：
```
./target/debug/ontok-propose \
  --schema ../.review-workspace/fix-props/schema.json \
  --names src/extract/table_names.json \
  --out <qa-req019b>/prod/proposal.json \
  --stats <qa-req019b>/prod/stats.json \
  --project extracted
```
实测：
```
stats.json    sha256 = 6b62a513765636c9440db4a916805bd725e1a51384b0cacbd4257e114ca029d6
              期望值 6b62a513765636c9440db4a916805bd725e1a51384b0cacbd4257e114ca029d6  ✅
proposal.json sha256 = b269f3436effda7c449afabecb6c48adc95257921c82b4ff9c19e5274b993e95
              期望值 b269f3436effda7c449afabecb6c48adc95257921c82b4ff9c19e5274b993e95  ✅
```
⇒ 本次修改**没有改变第 1 轮的逐字节产物**（Dev 关于「保持第 1 轮产物不变」的说法属实）。
附带确认 CLI 真实输出：91 表 / 142 items（实体 91 · 关系 51）/ 属性 964，
提案行仍打印 `（基版本 v1）`——round 1 下 `base_seq = DEFAULT_BASE_VERSION = 1`，正确。

## D-3 全量测试
见 05-full-test-runs.md（两次独立运行的真实数字）。
