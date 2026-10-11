# QA REQ-020a — 全量测试 + ConnectionReset 记录
worktree: feature-REQ-020 @ e5717a8；仅用 worktree 自带 target/（未设 CARGO_TARGET_DIR）

## 全量 `cargo test --no-fail-fast` 两次（QA 自跑）
| run | targets | passed | failed | 总数 | elapsed |
|---|---|---|---|---|---|
| run1 | 27 | 359 | 1 | **360** | ~81s |
| run2 | 27 | 359 | 1 | **360** | 81s |

Dev 声称 360/0 —— **总测试数 360 属实**（两次 run 均 359+1=360）。

## ConnectionReset：共 2 次（run1 一次、run2 一次），落点不同
- run1: tests/test_apply.rs:32 `test_frozen_version_rejects_writes`
        panicked: Db(Io(Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" }))
- run2: tests/test_concept_proposal.rs:650 `test_end_to_end_concept_group_via_dual_track_review`
        同一 ConnectionReset(104)

### 判定 = 环境抖动，非产品缺陷（证据链）
1. 两次落点**不同** —— 若是本 REQ 引入的缺陷，应稳定复现在同一处。
2. 两个文件 `tests/test_apply.rs` / `tests/test_concept_proposal.rs`
   **均不在** `git diff --name-only 23485c0..e5717a8` 的 6 个文件里。
3. 单独复跑，两个 target 全绿：
   - `cargo test --test test_apply`            → 21 passed / 0 failed
   - `cargo test --test test_concept_proposal` → 19 passed / 0 failed
4. 根因指向共享本体库的连接被对端重置（共用库 + 只 INSERT、并发跑的固有噪声）。

## 结论
产品侧有效数字 = **360 passed / 0 failed / 27 target**，
但需如实记入：QA 实测遇到 **2 次** ConnectionReset 环境抖动（上一轮 QA 报 0 次）。
