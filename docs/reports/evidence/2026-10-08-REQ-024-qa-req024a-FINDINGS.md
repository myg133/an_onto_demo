# REQ-024 独立验收结论（verifier，qa-req024a）

结论：**有条件放行**（无阻断项；2 条建议在合并前修，1 条属 BA 决策点）

## E · 全量与产物（全部独立复跑）
| 项 | Dev 声称 | 本人实测 | 判定 |
| --- | --- | --- | --- |
| 全量 3 轮 | 437 / 0 | 437/0、437/0、437/0，EXIT=0，32 suite | ✅ |
| ConnectionReset | 5 轮里 2 次 | **本人 3 轮 0 次** | ✅ 非本 REQ 缺陷 |
| 新增测试 | 基线 429 + 8 | `+#[test]`=8，`-#[test]`=0 ⇒ 429+8=437 | ✅ |
| stats.json sha | 6b62a513…29d6 | 6b62a513…29d6 | ✅ |
| proposal.json sha | b269f343…3e95 | b269f343…3e95（与 Dev artifact2 逐字节相同）| ✅ |
| fmt delta | 0 | 9 文件 parent/HEAD 逐文件对比，delta 全 0 | ✅ |
| 红线 3 文件 | 未动 | git diff 空 + sha 未变 | ✅ |
| fix-props/schema.json | 保留 | 219688 字节，未删 | ✅ |
| worktree | — | `git status --porcelain` 空 | ✅ |

注：`cargo fmt --check` 全仓报 55 个文件有差异（含本 REQ 未动的红线文件 view.rs /
drop_rate.rs / test_fk_naming.rs），无 rustfmt.toml ⇒ 仓库既有噪声，对本 REQ 无信号量。

## A · 能力静态化
判据判别力（本人以「同逻辑 + 变异文本」复现，未改 src）：
| 变异 | 判据结论 | 含义 |
| --- | --- | --- |
| 原样 | GREEN | 无误报 |
| 复原 `rebuildable` 翻转 | **RED** | ✅ 判据有判别力 |
| 配置派生能力（正当改动）| RED | ⚠ 过严 |
| `caps_for(self)` | GREEN | ⚠ 漏报 |
| `Self::caps_for(self)` | GREEN | ⚠ 漏报 |
| `(*self).rebuildable` | GREEN | ⚠ 漏报 |

Dev「行为级断言抓不住」的论证**独立成立**：parse.rs:374 `origin: None`、
parse.rs:394 `rejected_comments: None` 是全仓唯一两处构造点 ⇒ contract_gaps 恒非空
⇒ can_rebuild_proposal 恒 false ⇒ 翻转恒 Some(false) ⇒ capabilities() 返回值逐字节不变。

其他路径排查：AcpAgent 6 个字段无一被 capabilities() 读；trait 默认实现是静态
`Vec::new()`；src 内仅 2 个 impl（规则/ACP）均静态；登记表 capabilities 列只存不判。
**无其他运行时翻转路径。** 但 `capabilities()` 每轮被**独立读 4 次**
（orchestrate.rs:487 / 433、mod.rs:404）——这正是该不变量消灭的危险形状。

## B · 抽取后守卫
- 错误信息：点名组 `table:t_dev`、表名、缺失字段、已声明能力、时机理由 ⇒ 说人话。
  第 1 组即拦（3 组表中已抽组数 = 1），未继续烧钱。
- ⚠ 3 处 `n` 字面量笔误（见 F1）。
- B.4 过严性：一组合法返回空 types ⇒ **整轮崩溃**（实测）。
- `bail!` vs 规格 §7.1 ⑤：**不违反**（详见报告正文判定）。

## C · fail-closed（8/8 全部 fail-closed，零回落）
无 endpoint / 空串 / 空白 / 不存在的可执行文件 / `/bin/false` / `/bin/true` /
未知协议 a2a / 空协议 ⇒ 全部 Err。反向对照：`inproc` 仍造得出来 ⇒ 失败确属 acp 分支。
不存在的可执行文件报错含 `No such file or directory (os error 2)`，且明写「没有回落到进程内规则实现」。

## D · 契约实现唯一性 + 两处补漏
- 单一实现：`acp::contract_gaps` 与 `assemble::contract_gaps` **函数指针相同**
  （均 0x5615420686c0）⇒ 铁证只有一份函数体。
- 承重性：缺 nullable ⇒ 真判据 1 条 / 去掉该判据 0 条；缺 links[].evidence 同理。
  齐备候选 ⇒ 零缺口（守卫不误伤合法答案）。

## 发现清单（按严重度）
- **F1 [低-中] 守卫错误信息有 3 处 `n` 字面量笔误**
  `src/agent/orchestrate.rs:663,665,666`（blame 40b755d，本提交新增 3 处）。
  实际输出含 `，n` + 31 个空格。同类笔误在 508/510-514 已存在（父提交即有，非本 REQ）。
- **F2 [中 · BA 决策点] 守卫可能过严：一组答案为空 ⇒ 整轮崩溃**（实测）。
  今日生产零影响（无 src 实现声明 REBUILDS），但对未来的重建型实现是真实风险。
- **F3 [低] 判据①比它声称的不变量更严**（拦下正当的配置派生能力）。
- **F4 [低] 判据①有 3 条绕过路径**（`caps_for(self)` / `Self::caps_for(self)` / `(*self).x`）。
- **F5 [低] 「不存在的可执行文件」应补测**，Dev 的理由（要真起子进程）不成立：
  ENOENT 在 fork 前就失败，零子进程成本；`/bin/false` 亦 <1s、无依赖、可复现。
- **F6 [信息] 不变量只覆盖 AcpAgent**，未来第三个实现（RN-5）不受该判据保护。
