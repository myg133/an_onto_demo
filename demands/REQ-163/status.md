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

## QA 结论（2026-10-27 20:05）—— **可以合，0 阻断**

报告 `BA\.local\qa\REQ-163\REPORT.md`。

| 组 | 结果 |
| --- | --- |
| A 真不碰库 | 4/4：三条 env **unset** 跑 7/7；QA 加做「**不删 env**、把坏库留在环境里跑真二进制」也到评审台；**有真库时 `projects|versions 0|0 → 0|0`**（changelog 原来这半条是 SKIP，由 QA 补实测关闭） |
| B 逐字节相同 | 同 `built_at` ⇒ 数组相等、sha 相同；**阴性对照**：不给 `--built-at` ⇒ 不同 sha（证明判据不是空转） |
| B3 变异 | 「CLI 自己拼 `PackInput` + 塞 `builder_host`」⇒ **红**（1199 vs 1168 字节） |
| C 顺序变异 | `serve` 提前 ⇒ 红；`write_draft` 挪进 ①② 段 ⇒ 红；`write_draft` 留在原函数但提到最前 ⇒ **也红**（顺序断言本身有牙） |
| C4 | 插 `PgApplier` ⇒ 红；插 `axum::` ⇒ 红 |
| E 回归 | FEAT 侧 QA **独立**跑全量：1995/225/122，与 Dev 声明逐数命中；**225 里本单碰过的套件 0 命中**；唯一非环境失败在基线复现（同文件同行同断言） |

## 我拍板的三条

| # | QA 发现 | 裁定 |
| --- | --- | --- |
| F3 | `--bind` 非回环要等 ①②③ 跑完（白烧 token）才拒，且 exit 1 | **修**（挪进入参闸、exit 2、断言 work-dir 零产物） |
| F4 | 报错文案教人加 `--allow-public`，但 REQ-163 已删该开关 | **修**（文案改成「本命令只绑回环，无放行开关」） |
| F1 | `strict_deterministic` 被关，但改回 `true` 实测仍绿 | **修**（改回 `true`；若真红则修夹具而不是关闸） |
| F6 | `--base-version` 只校验 `> 最新` | **不改**（允许更大值是给"从别的分支接续"留的口子；记档即可） |
| F7 | R2 只有行为面判据，无"必须调 `pack_version`"的结构守卫 | **不改**（忠实复刻不改变产物 ⇒ 不红是对的；塞 `builder_host` 才会红，那才是真实漂移） |
| F2 | changelog 说 `import --file` 一步进库 | **我改了**（实测需两步：先 `version new`，否则报「版本不存在」**并留下空壳项目**） |
