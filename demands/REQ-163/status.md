# 状态：REQ-163 · 构建端「纯本地链」收口

## 当前

**待验证**（Dev 3 commits：`f888100` 打包点统一 / `3ea7d41` build run 纯本地 + `bundle pack --dir` / `d341bae` 判据；未 push；待 QA）

## 我复核过的（lion 亲跑）

| 判据 | 结果 |
| --- | --- |
| `cargo check --lib` | 绿 |
| `--test test_req163_local_chain` | **7 / 7** |
| `--lib review_open::build_run` | **11 / 11** |
| `--test test_req160_review_open` | 9 / 9 |
| `--test test_req162_console` | 12 / 12 |

## Dev 交付的关键证据（待 QA 复验）

| 项 | 值 |
| --- | --- |
| `build run` 真进程（三个 env 被 `env_remove` 删掉） | 草稿写出 `<work>/out/proposal.json`（1 条，项目 `demo_workspace`），服务起在 `127.0.0.1:46307` |
| 工作目录故意用 `Demo Workspace`（空格+大写） | 项目名 sanitize 成 `demo_workspace`，与 `console serve --dir` 同一份实现 |
| `bundle pack --dir` vs 页面导出 | **1668 字节同 sha256 `12200d93…`**，verify exit 0 |
| 旧判据替换 | `the_preparatory_write_happens_after_both_stages` → `the_draft_write_and_the_desk_serve_happen_after_both_stages`（4 条可红断言） |

## 待办

- [ ] QA（重点：**两个顺序变异是否红**、逐字节相同是否真成立、6 处偏离判断、「全量零回归」抽验）
- [ ] 合并后：skill 里那条兼容提示（`build run` 尾步可能落到旧 `review open`）**要删掉** —— REQ-163 之后它不成立了
- [ ] 合并（待用户）
