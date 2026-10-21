# 开发状态 · REQ-114 页面图交互升级

> worktree `feature-REQ-114` · 分支 `feature/REQ-114` · 基线 `develop` `e9755e8` · **未 push**
> 本轮性质：**QA 首轮判 FAIL（阻断 P0：探针无牙）→ 只修探针 + 三处文档 + 两条非阻断**，产品逻辑不再动

- **需求**：REQ-114 · 页面图交互升级（AC-001~AC-011）
- **当前任务**：布局探针改为**行为级**（无斥力场景 + 剥注释计数）已完成，**待 QA 复审**
- **状态**：进行中（PR 与状态机流转由 BA 决定，Dev 不发布）
- **日期**：2026-10-22

## 本轮改了什么（QA FAIL 后的修复轮）

| # | 项 | 文件 | 一句话 |
| --- | --- | --- | --- |
| **A-1** | 探针**行为级**化 | `tests/fixtures/req114/layout_probe.js` | 新增**无斥力场景**（`REP=0`）：83 节点同坐标时「不重叠」只可能由 `collide` 兑现 ⇒ 删一行 / 注释 / `if(false)` 全都会红 |
| **B** | 静态判据**剥注释 + 恰好 2 处** | 同上 | 先剥 `//` 与 `/* */` 再数 `collide(ns, GAP);`，要求 `=== 2`（原来是裸正则 `>= 1`，注释掉的文本照样被数到） |
| **C1** | `pass()` 假绿 | 同上 | 起点判据原来用 `pass(name, 三元字符串)` —— 条件为假也 PASS；改成 `expect(cond, …)` |
| **C2** | 边数没断言 | 同上 | 加 `edges === 52`（两个场景各一条） |
| **C3** | 内核注释里的探针路径过期 | `src/server/page.rs:223` | `.review-workspace/req114/layout_probe.js`（不入库）→ `tests/fixtures/req114/layout_probe.js`（入库） |
| **D1** | `PROVENANCE` 把旧 H1 说成「b714f48 那一份」 | `tests/fixtures/req107/golden/PROVENANCE.txt` | 改成「REQ-110 收口后在 develop 上的静态壳」，并写明 b714f48 那个 worktree 里根本没有 golden |
| **D2** | `PROVENANCE` 说已知限制「写进了交付物」 | 同上 | 改成实际位置：`page.rs` 模块文档「已知限制」小节 + 探针文件头「边界」段 |
| **D3** | `.feature/*` 还是旧需求 | `.feature/status.md` / `verification-report.md` / `manifest.json` | 三份全部写成本需求（REQ-114） |

⚠️ C3 改的是 `INDEX_HTML` **里面**的注释 ⇒ 页面字节变了 ⇒ golden 又重锚一次（`1f86ea…` 32276 → `d8bd4581…` 32396），`SHA256SUMS` 与 `PROVENANCE` 同步。

## 判据实测（摘要，细节见 `verification-report.md`）

| 判据 | 实测 |
| --- | --- |
| 基线探针 | `ALL PASS（场景 1：168 帧 / 余量 15.79px；场景 2：168 帧 / 余量 5.20px）` **exit 0** |
| M1a 两行删一行 | 静态层 `1 ≠ 2` + 场景 2 余量 `-30.94px` ⇒ **exit 1** |
| M1c 两行改成注释 | 剥注释后 `0 ≠ 2` + 场景 2 余量 `-32.00px` ⇒ **exit 1** |
| M1d 两行包进 `if(false)` | 静态层仍 2 ⇒ 由**场景 2** 抓住（余量 `-32.00px`、包围盒 `0 × 0`）⇒ **exit 1** |
| M2 `min=0`（collide 空转） | 单元 2 条 + 场景 2 ⇒ **exit 1** |
| M3 改 `END` 标记 | `die()`：找不到标记 ⇒ **exit 1** |
| 每次变异后还原 | sha256 `9E5ABADF…` 前后一致（逐字节） |

## 测试

```text
lib 245 · doc 4 · 集成 72 个目标（774 用例），4 个目标红且全部与本需求无关：
  drop_rate / test_fk_naming     → 缺脱敏 schema.json（fail-closed 输入不在版本控制内）
  test_agent_orchestration       → 在基线 page.rs 上同样红（已实测回退复现）
  test_req107_red_lines          → 绕行产物（临时挪开 test_retained_names 才会红；单跑 2 passed）
test_retained_names_req084（6 条）→ Windows 上编译不了（include_str! 那个 Unix-only 的 bin）
页面/守门/golden 专项全绿：pipeline_bytes 1 · zero_visible_change 3 · graph_server 9 ·
  static_texts 3 · red_lines 2 · req110_ttl 15
```

> 口径说明：本机 `cargo test` 的「目标」数含全部 `tests/*.rs` 与 bin 单测（72），与 REQ-112 报告里
> 「43 套件」不是同一口径，别直接比数字。

## 如实登记（AC-011）

1. **未做真浏览器手测**（本机无 headless 浏览器，也不许为此加依赖）；
2. **标签宽度是估算**（`length × 11`），没有 `getComputedTextLength()`；
3. **力导向参数是 Node 扫参得出的**（`REP=400` / `CENTER_F=0.4` ⇒ 83 节点收在 ~529×528），
   真实数据下的观感没有在浏览器里看过。

---

# 历史：REQ-112 审核界面接入 AI 建议（B-1 阻断修复轮）

> 以下为上一需求原文，保留备查。

> worktree `feature-REQ-112` · 分支 `feature/REQ-112` · 基线 `444080f`（QA 首轮 FAIL）· **未 push**
> 本轮性质：**阻断修复（B-1）+ 两条非阻断（N-2 / N-3）** —— 不扩大范围

- **需求**：REQ-112 · 审核界面接入 AI 建议（AC-001~AC-010）
- **当前任务**：B-1 阻断 + N-2 白名单 + N-3 交付物 已改完，**待 QA 复审**
- **状态**：待验证（PR 与状态机流转由 BA 决定，Dev 不发布）
- **日期**：2026-10-21

## 本轮只改了三处（其余一行未动）

| # | 项 | 文件（`path:line`） | 一句话 |
| --- | --- | --- | --- |
| **B-1** | 冲突规则不可达 | `src/domain/confidence.rs:1559-1571`（判据在 `:1575 match c.band` **之前**） | `matched == sampled && sampled > 0 && fit < 0.50` ⇒ `conflict`，**不限档位** |
| **N-2** | 两条批量路径各有一份白名单 | `src/review_ui/http.rs:517`（`batch_gate`）+ `:598` / `:631` 两个调用点 | `by=confidence` 也过 `enters_batch()`；`conflict`/`none`/`review`/`reject` 一律不进 |
| **N-3** | 交付物仍是旧需求 | `.feature/verification-report.md` / 本文件 | 报告写 REQ-112 段（REQ-111 原文保留在下方），A-3 两条如实登记 |

附带一处**口径同步**（N-2 的直接后果）：`src/review_ui/page.rs:436-442`，「按档位」弹窗把
`冲突 / 人工核对 / 拒绝` 三类被排除的条数也说出来 —— 否则「将接受 N 条」会让人以为剩下的也被一起接受。

## 判据实测（摘要，细节见 `verification-report.md`）

| 判据 | 实测 |
| --- | --- |
| 结构全 1.0 + 5/5 + fit=0.0（生产评分函数） | `score=0.85` / `band=middle` / `kind=conflict`（**不是** `lean_accept`） |
| 该条在 `/api/preview?by=suggestion` 的排除里 | 真 serve：`count=2 items=[3,4] excluded.conflict=1` |
| 边界 `fit=0.50` / `4/5` / `sampled=0` | 分别是 `review` / `lean_accept` / `reject` —— **都不是 conflict** |
| N-2：`min_confidence=0.80` | 真 serve：`items=[3,4]`，`score=0.85` 的冲突**不在**白名单 |
| 真实数据不受影响 | 135 条：`accept 29 / lean 9 / review 14 / none 83 / conflict 0`；`by=suggestion` 仍是 **38** |

## 测试

```text
lib 245 · doc 4 · 43 套件 422（基线 416 + 本轮新增 6 条 B-1/N-2 用例）
test_req112_suggestion 27（原 21 + 6）· test_req111_review_ui 14（不变）
两处既有红灯归因不变（test_retained_names_req084 绕行必编不过 / test_agent_orchestration 的 CLI 端到端）
```

## 如实登记（A-3 / A-7）

1. **当前数据集无冲突样本** —— 上面的冲突条目是**构造样本**（`dev112c`），不是真实数据观测；
2. **冲突分组「排最前」只有源码级断言**，**无浏览器验证**（本机无 headless browser）。

## 临时数据（未清理，本机无 psql/删除命令）

`anspire_real` 库新增 3 个项目：`dev112fix`（135 条，真实 p1 副本）/ `dev112c`（6 条，B-1 构造）/ `dev112b`（3 条，边界构造）。
