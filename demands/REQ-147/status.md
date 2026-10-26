# 状态：REQ-147 · `doc ingest` 接 ACP + 通用抽取 skill

## 当前状态

**进行中 · 抢救版待 QA**（2026-10-26）

> 通路已实现并提交（`b99c914`），但 BA 裁定的 3 件补件未完成 ⇒ **不能进 QA**。

## C 方案（2026-10-26）

原分支落后 develop **166 个提交**，rebase 会撞。用户拍板 **C 方案**：

| 做法 | 结果 |
| --- | --- |
| 推送原分支留档 | ✅ `origin/feature/REQ-147`（3 提交） |
| 只捞核心 | `doc_acp.rs`(973) + 测试(656) + 夹具 → 新分支 `feature/REQ-147-salvage`（基线 `1137ea8`） |
| CLI 按新结构重写 | `DocCmd::Ingest` 加 5 参 + `build_doc_transport` 唯一岔口 |
| 提交 | `0961e70`（1 个，7 files / +2212 / −13） |

**BA 自查（已验证）**：
- `cargo check --lib` 通过（模块与当前 `doc_ingest` API 兼容，语义未漂）
- `strings ontok-builder | grep 'acp:'` = 1 · `strings ontok | grep 'acp:'` = **0** · `strings ontok | grep X-TC-Action` = **0**
  ⇒ **只进构建面，服务面仍干净**

## 进度（2026-10-25 21:15 实测）

| 项 | 状态 | 证据 |
| --- | --- | --- |
| 通路实现（`--agent acp` 四参数 + `doc_acp` 模块 + 18 条单测 + 6 条端到端） | ✅ 已提交 `b99c914` | 与 develop 差 1 个提交 |
| **补件 R-2**：`--strict-deterministic` + "N 块失败"提示 | 🟡 **在做** | 源码已出现 `strict-deterministic` 4 处；工作区 4 个未提交文件 |
| **补件 R-3**：同模型对照（`session/set_config_option` 切 deepseek） | ❌ 未开始 | grep `set_config_option` = **0** |
| **补件 R-1**：按宽口径重报 AC-006 | ❌ 待做 | —— |
| QA | ⬜ 未派 | 补件没完不派 |

## 第一轮交付的事实（供对照）

| 指标 | 内置 LLM | ACP + skill |
| --- | --- | --- |
| 断言 | 809 | 2,352 |
| 关系 | 252 | 989 |
| **泛动词占比** | 18.65% | **1.31%** |
| 关系两端都是概念 | 26.2% | **96.4%** |
| quote 校验通过 | 100% | 98.7%（**31 条编造被机器丢掉**） |

⚠️ **但对比混了模型变量**（ACP 侧 `MiniMax-M2.7`，内置侧 `deepseek-v4.1-flash`）——
⇒ **"skill 本身值多少"尚未证明**。这正是 R-3 要求同模型对照的原因。

## 我写错的一条判据

AC-006 原文「`belongs_to` 命中率 > 0」**结构性不可满足**：
`STRUCTURAL_PREDICATES`（`doc_ingest.rs:145`）在计数**之前**就把它丢了。

⇒ 已改宽口径「归属/包含类占比 > 5%」（1.19% → 32.76%）。

**教训：判据既要能红，也要能绿。** 我当时只验了前者。

## 三条裁定（见 acceptance §三）

| # | 裁定 |
| --- | --- |
| R-1 | AC-006 改宽口径（我的判据错） |
| R-2 | AC-003 采纳 A+C（屏幕提示 + `--strict-deterministic`）；⛔ 不采纳"失败进缓存" |
| R-3 | **对照实验必须分离模型变量**，否则结论降级 |
| R-4 | skill 改为**语义契约**（不规定输出形状），消除两套形状冲突 |
| R-5 | `ontok` 自身名不算 R4 违规；`--agent-timeout-secs` 是合理新增 |

## 待办

- [ ] Dev 补 3 件
- [ ] QA（重点：strict-deterministic 能否无条件保证确定性；同模型对照是否真做了）
- [ ] 合并 → 用 ACP 重跑 30 类本体的抽取，看质量是否真的更高
