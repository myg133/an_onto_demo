# REQ-047 验收 · 中间落盘（会话已多次丢失，边做边落盘）

worktree: /home/myg133/Projects/Anchnet/Ontology/verify-REQ-047
branch: verify/REQ-047  HEAD d521f79  baseline e0f6553
临时目录: .review-workspace/verify-req047/

## 0. 交付面
- git status 干净；HEAD = d521f79（4 commits: fff0938, 5392a4b, 191fa68, d521f79）
- 改动面 = `git diff --numstat e0f6553..d521f79`：tests/common/mod.rs +3/-1、
  tests/common/source_root.rs +154/-27、tests/test_anchor_guard.rs +590/-0
  ⇒ 合计 +747/-28，**与 Dev 自述一致**
- 红线：`src/` 零改动（`git diff --name-only e0f6553..d521f79 -- src/` 为空）
  `tests/drop_rate.rs`、`tests/test_fk_naming.rs` 零改动
  diff 中无 `ignore`、无被删的 `fn`（3 处 `-fn` 全在 source_root.rs 且为签名重构）

## 1. 环境类失败二分（关键）
- 首次全量：**125 failed**，退出码 101。逐条 panic 位置分类：
  113× `tests/common/mod.rs:72:50`，其余分散
  ⇒ **全部是 `MissingEnv("ONTOK_TEST_DATABASE_URL")`**，属**环境类失败**，非代码失败。
- 处置：从仓库根 `.env` 取该变量、库名 `notol`→`ontol`，只经环境变量传入
  （脚本 `run_with_db.sh`，口令不落盘、不打印）
- ⚠️ 口径记录：**125 = 环境类，不是「代码失败」**。不得用它冒充任何变异被杀死。

## 2. 已核实通过的项

### 2.1 报错文本 byte-identical（G-3 前提）
- `diff -w` 两棵树的 `missing_file_diagnostic` / `mismatch_diagnostic`：
  **唯一差异 = 抬头换成 `{header}` + 具名参数**（`header = header_missing_file()` /
  `header = header_mismatch()`），正文逐行未动。
- 按 Rust 字面量规则求值后（`\` 续行拼接 + `\n` 还原）：
  **A 抬头 byte-identical = True；B 抬头 byte-identical = True**
- 抬头之后的 `│\n\` 行在 head 侧紧跟 `{header}` 保留 ⇒ 整条报错文本不变。
  ⇒ **Dev 的「byte-identical（only relocated）」成立。**

### 2.2 G-3 零 set_current_dir
- `grep -rn set_current_dir tests/` 命中 **仅 2 处，且都在注释里**
  （test_anchor_guard.rs:26 `//!`、:171 `///`）⇒ **零实际调用**。

### 2.3 「代码 7 / 注释 4」登记订正（c98a631 基线）
`git grep -n CARGO_MANIFEST_DIR c98a631 -- tests/` = **11 处**，逐行定性：
- 代码 7：test_acp_extraction.rs:376、test_agent_orchestration.rs:1150、
  test_connect_retry.rs:306、test_extraction_agent.rs:1107、
  test_snapshot_write_paths.rs:377/408/886
- 注释 4：test_connect_retry.rs:46/55/270/273
⇒ **代码 7 / 注释 4 / 合计 11，独立复现，与登记一致。**

### 2.4 F-5 撤销依据
- 当前树 d521f79：`:265 Err(e) => last_err = Some((root.path.clone(), e))`
  `:274 let (root, e) = last_err.expect(...)`
- ⇒ **root 与 e 确为同一元组**，结构上不可能不一致 ⇒ **F-5 撤销的实质成立**。
- ⚠️ 文档写 `:264/:273`（source_root.rs:85-86），实际是 **265/274，差 1 行**。
  该文档已自披露「行号会随编辑漂移，以代码为准」⇒ 记为**轻微文档偏差，非阻断**。

### 2.5 基线同步（AC-004）
- `eed0c13` 只在 `demand` 分支，未进本 worktree 的基线（`git merge-base --is-ancestor eed0c13 d521f79` = NO）
- 该提交 6 个文件、全部在 `demands/*/acceptance.md` + `dispatch/req-registry.md`，
  **未出现 docs/reports 或 evidence** ⇒ **历史 report 一行未动（成立）**
- `git grep '预期结果：\*\*536' demand -- 'demands/*' 'dispatch/*'` ⇒ **空，无残留**
- 5 处「预期结果」全部改为 539 且**每处**都写了「⚠️ 基线由 536 变为 539，原因：新增 G-1/G-2/G-3 三条守卫」
  （REQ-043:106、REQ-044:94、REQ-045:106、REQ-046:85、REQ-047:85）
- 历史 536 引用均标注「为**当时**实测，作历史记录保留」⇒ **口径处理正确**
- 仓库 **无 remote**（`git remote -v` 空）⇒ 未 push，成立

## 3. 已发现的疑点（待实测确认）

### 3.1「朴素 grep 8 处」对不上当前树
- 当前树 `grep -rn CARGO_MANIFEST_DIR tests/` = **23 处**（不是 8）
- e0f6553 树 = **11 处**
- c98a631 树 = **11 处**
⇒ Dev 的「8」应是**开发中途某版本**的数字，**在交付树上不可复现**。
- 但**结论方向**成立：剥注释后，真正构成 `env!(` + 实参命中的**代码行只有 1 处**
  （source_root.rs:104 锚点行）。23 处里 17 处是注释、其余是字符串字面量里的 `CARGO_MANIFEST_DIR`
  字样（不构成 `env!(` 窗口命中）。

### 3.2 G-1 文档自称覆盖 `std::env::var(...)`，实现可能不覆盖
- test_anchor_guard.rs:480-483 的文档写「裸 grep 会被三样东西绕过：
  2. `std::env::var(...)` / `option_env!(...)`」
- 实现锚点是 `['e','n','v','!','(']` ⇒ `std::env::var(` **不含 `env!(`**，静态判断**不会命中**
- ⇒ 若实测确认，则「`option_env!` 覆盖」成立但「`std::env::var` 覆盖」**不成立**，
  属**文档过度声称**（非阻断，但影响「词法级」的覆盖承诺）。

### 3.3 ⚠️ 可能存在「第四种改法」同时躲过 G-2 三层（待实测）
G-2 断言的是**来源标签与次序**，不是 Cwd 候选的**取值**。因此：
```rust
fn crate_root_from_cwd() -> Option<PathBuf> {
    Some(PathBuf::from(BUILD_MANIFEST_DIR))   // 保留 Cwd 标签，值换回编译期常量
}
```
- (a) 纯函数 `order_candidates` 未被改动 ⇒ 通过
- (b) 同上 ⇒ 通过
- (c) `real[0].origin == Cwd` ⇒ **仍然通过**（标签没变）
- G-1/G-3 与此无关 ⇒ 通过
⇒ 即 **G-2 抓不到「保留标签、换掉来源」这一种还原方式**。
待实测确认，暂记为**待确认的判据缺口**。

## 4. 全量（基线 d521f79，带库）

- 命令：`.review-workspace/verify-req047/run_with_db.sh cargo test --no-fail-fast`
  （run_with_db.sh 只从仓库根 `.env` 读 ONTOK_TEST_DATABASE_URL 并把库名 notol→ontol）
- **539 passed / 0 failed / 1 ignored，退出码 0**
- 口径：`awk '/^test result:/ {p+=$4; f+=$6; i+=$8}'`（**未用 paste -sd+**）
- 二进制数 = `grep -c '^test result:'` = **37**
- 耗时：`real 9m51s`（Dev 自述 5m15s；共享集群负载浮动，**不声称稳定值**）
- 唯一 ignored = `todo_identifier_defenses_have_own_assertions`（原有，非新增）
- ⇒ **基线数字与 Dev 自述一致**

## 5. G-1 绕过矩阵（独立构造，探针 `tests/qa_g1_probe_tmp.rs`，跑完已删）

口径：探针自身 `cargo test --test qa_g1_probe_tmp` = **16 passed**（先证明能编译能跑，
否则「编译失败」会被误当成「判据行为」）；再跑 `cargo test --test test_anchor_guard`
= **3 run / 1 failed**，从失败正文里 grep 出被判违规的行号，与探针已知行号比对。

| 探针 | 写法 | 代码行 | G-1 | 判定 |
|---|---|---|---|---|
| P1 | 直白 env! | 7 | 命中 | ✅ 抓到 |
| P2 | 带空格 `env!( "…" )` | 13 | 命中 | ✅ 抓到 |
| P3 | 跨行实参 | 19 | 命中 | ✅ 抓到 |
| P4 | 括号后接逗号 | 27 | 命中 | ✅ 抓到 |
| P5 | `option_env!` | 33 | 命中 | ✅ 抓到 |
| P6 | `r#"…"#` | 39 | 命中 | ✅ 抓到 |
| P7 | `r###"…"###` 多井号 | 45 | 命中 | ✅ 抓到 |
| P8 | 宏嵌套 `concat!(env!(..),…)` | 51 | 命中 | ✅ 抓到 |
| P9 | `std::env::var("CARGO_MANIFEST_DIR")` | 57 | **未命中** | ⚠️ **漏网** |
| P10 | `env!(concat!("CARGO_MANIFEST_","DIR"))` | 63 | **未命中** | ⚠️ **漏网** |
| P11 | 实参离 `env!(` 超过 40 字窗口 | 69 | **未命中** | ⚠️ **漏网** |
| P12 | `use std::env as e;` 后 `e::var(...)` | 75 | **未命中** | ⚠️ **漏网** |
| P13 | 行注释内 | 82 | 未命中 | ✅ 正确忽略 |
| P14 | 生命周期 `<'a>` + 行注释 | 88 | 未命中 | ✅ 正确忽略 |
| P15 | 块注释内 | 94 | 未命中 | ✅ 正确忽略 |
| P16 | **嵌套**块注释内 | 100 | 未命中 | ✅ 正确忽略 |

结论：
- **词法级剥注释属实**：行注释 / 块注释 / **可嵌套**块注释 / 生命周期后的行注释，
  四种都正确剥掉，**零误报**；字符串字面量**保留为代码**（否则 P1 会漏）。
- **8 条真实绕过全部抓到**，与 Dev 自检样本口径一致（且多井号 raw string 也抓到）。
- **4 条漏网**：
  - P9/P12 根因：匹配锚点是 `['e','n','v','!','(']`，`env::var(` 不含 `env!(`
  - P10 根因：窗口内要出现**连续子串** `CARGO_MANIFEST_DIR`，`concat!` 拆开会打断
  - P11 根因：`ARG_WINDOW = 40` 是硬上限
- ⚠️ **文档过度声称**：test_anchor_guard.rs:480-483 写「裸 grep 会被三样东西绕过：
  2. `std::env::var(...)` / `option_env!(...)`」—— 实测 **`option_env!` 覆盖、
  `std::env::var` 不覆盖**。属**非阻断的文档缺陷**。
  （注：`std::env::var` 是**运行期**读，语义上不等于编译期常量，
  严重度低；但文档承诺没兑现。）

## 6. M1 · 二进制级（已实跑）

- 变异：`candidate_roots()` 的 `order_candidates(crate_root_from_cwd(), …)` → `order_candidates(None, …)`
- 编译：**成功**（`#![allow(dead_code)]` 兜住未用的 `crate_root_from_cwd`）⇒ 非编译失败
- 二进制 `test_anchor_guard`：**3 run / 1 failed**
- 失败点 = `tests/test_anchor_guard.rs:106` 的断言 **(c) 真实接线**：
  `left: BuildManifest / right: Cwd`
- ⇒ **(a)(b) 纯函数断言确实碰不到这处接线，只有 (c) 杀得掉**，
  Dev 对三层分工的说法**经实测成立**。

## 7. M1 · 全量级（**本需求成立的核心证据**）

命令：`run_with_db.sh cargo test --no-fail-fast`（M1 已施加，编译成功）

| 口径 | 基线 d521f79 | **M1 施加后** |
|---|---|---|
| passed | 539 | **538** |
| failed | 0 | **1** |
| ignored | 1 | **1** |
| 二进制数 | 37 | 37 |
| 退出码 | 0 | **101** |
| 耗时 | 9m51s | 5m58s |

- **失败二进制只有 1 个**：`tests/test_anchor_guard.rs`（`test_anchor_guard-32c387578ef55620`）
- **失败测试只有 1 条**：`anchor_order_is_cwd_first_and_walk_is_bounded`
- 538+1+1 = 540 = 539(基线 passed)+1(基线 ignored)，**539−1=538 完全对账**
- 上一轮 QA 实测 M1 = **536/0/1，退出码 0** ⇒ **现在 M1 不再存活。核心要求达成。**

## 8. 变异测试总表（全部实跑，编译成功才计）

| # | 变异 | 二进制 | 运行条数 | 结果 | 杀死它的判据 |
|---|---|---|---|---|---|
| M1 | `candidate_roots()` 的 cwd 分支删掉（`crate_root_from_cwd()`→`None`） | `test_anchor_guard` | 3 run / 1 failed | ✅ **杀** | G-2 断言 (c) `test_anchor_guard.rs:106` |
| M1-全量 | 同上，跑全量 | 37 个 | 538/1/1 | ✅ **杀** | 仅 `test_anchor_guard` 失败 |
| M2 | `header_mismatch()` **完整**换成类别 A 抬头 | `test_anchor_guard` | 3 run / 1 failed | ✅ **杀** | G-3 `assert_ne!` `test_anchor_guard.rs:182` |
| M2-伪 | B 抬头**只换首句**、保留自己的【失败类型】 | `test_anchor_guard` | 3 run / **0 failed** | ⚠️ **存活** | —— |
| M3 | 在 `tests/test_acp_extraction.rs:29` 新增裸 `env!("CARGO_MANIFEST_DIR")` | `test_anchor_guard` | 3 run / 1 failed | ✅ **杀** | G-1，报 `tests/test_acp_extraction.rs:29` |
| G1-EMPTY | 让实扫跳过锚点文件（判据恒空、自检样本仍过） | `test_anchor_guard` | 3 run / 1 failed | ✅ **杀** | G-1 反空转断言，报「🔴 判据空转」 |
| **M1b** | `crate_root_from_cwd()` 改成 `Some(BUILD_MANIFEST_DIR)`：**保留 Cwd 标签、只换取值** | `test_anchor_guard` | 3 run / **0 failed** | 🔴 **存活** | —— |

### 存活变异逐条论证

**M2-伪（只换首句）＝ 等价变异，不是判据有洞。**
抬头仍以「【失败类型】**二进制与当前目录不匹配** —— **这不是「源码文件被删了」**」收尾，
判别短语仍在，两类仍可区分；事故里「把目录不对读成文件被删」这一危害**没有复现**。
⇒ **Dev 自述 (d) 属实**：他那次「伪存活」是变异脚本不完整，不是判据漏网。
我用完整替换重跑，M2 确实被杀（见上表）。

**M1b ＝ 判据有洞（真实缺口，已实测确认）。**
- 根因：G-2 断言的是 `RootOrigin` **标签与次序**，**从不校验 Cwd 候选的取值**。
- M1b 让 `candidate_roots()` 返回 `[Cwd(path=BUILD_MANIFEST_DIR), BuildManifest(path=同一目录)]`
  ⇒ 标签仍是 Cwd、下标仍是 0 ⇒ 断言 (a)(b)(c) 全过；G-1/G-3 与锚点来源无关。
- **功能后果 = REQ-046 的修复被完全还原**（`read_source` 重新只读编译期目录）。
- 严重度：**低-中**。M1b 需要刻意写 `Some(PathBuf::from(BUILD_MANIFEST_DIR))`，
  不是「顺手还原」那种自然改法（自然改法 = 删分支，已被 M1 证实能杀）。
  但它属于「看起来对、其实错」，比显式还原更隐蔽。
- 廉价修法建议：加第 (c2) 层 —— 断言 `real[0].path` 与 `crate_root_from_cwd()` 的取值
  **不等于** `BUILD_MANIFEST_DIR` 之外的某种「可伪造」值是做不到的（cargo test 下二者本就相等），
  更可行的是把 `crate_root_from_cwd` 变成 `crate_root_from(&std::env::current_dir().ok()?)`
  的**内联调用点**，取消这个可被单独替换的函数（结构上不给「换取值」留位置）。

## 9. 「判据恒空」防线 —— 独立验证成立

变异 G1-EMPTY 让 `collect_test_sources` 跳过 `source_root.rs`：
- 9 条自检样本**全部仍过**（它们不经过目录遍历）
- violations 为空（别处没有裸 env!）
- ⇒ 若无反空转断言，G-1 会**全绿而实则不拦任何东西**（正是 M1 的同一种病）
- 实跑结果：断言触发，正文为
  `🔴 判据空转：一条 ANCHOR-ALLOW 都没认出来，G-1 等于没写`
⇒ **`allowed_hits >= 1` 这条真的能防住「判据恒空」，Dev 的自检设计属实。**

## 10. 同会话基线对照（e0f6553）

做法：`git checkout e0f6553 -- tests/` + 删掉 `tests/test_anchor_guard.rs`
（`git diff --quiet e0f6553 -- tests/` = IDENTICAL），用**同一个 target**跑全量，
跑完 `git checkout HEAD -- tests/` 还原。

| 口径 | 基线 e0f6553 | **HEAD d521f79** | 差 |
|---|---|---|---|
| passed | **536** | **539** | **+3** |
| failed | 0 | 0 | 0 |
| ignored | 1 | 1 | 0 |
| 二进制数 | 36 | 37 | +1 |
| 退出码 | 0 | 0 | — |
| 耗时 | 6m23s | 9m51s | 负载浮动，不声称稳定值 |

交叉印证：`git grep -c '#\[test\]' <ref> -- tests/` 求和 = **295 → 298 = +3**
⇒ **基线由 536 变 539 的原因确实是「新增 G-1/G-2/G-3 三条守卫」，数字自洽。**

## 11. 红线核查

| 红线 | 结论 | 证据 |
|---|---|---|
| `src/` 零改动 | ✅ | `git diff --stat e0f6553..d521f79 -- src/` 为空 |
| `tests/drop_rate.rs` 零改动 | ✅ | `--name-only` 查询为空 |
| `tests/test_fk_naming.rs` 零改动 | ✅ | 同上 |
| `test_humanize_still_refuses_unknown_columns` 断言未放宽 | ✅ | `src/domain/view.rs:1254` 四条反例 `blob_ref_xyz` / `mystery` / `zzz_unmapped_token` / `aisearch_times_internal_v2` 全部在位；`src/` 零改动 |
| 无测试被删除 | ✅ | diff 中 3 处 `-fn` 全在 `source_root.rs` 且为签名重构，非测试 |
| 无 `#[ignore]` 新增 | ✅ | `#[ignore` 计数 e0f6553=2 → d521f79=2；全量唯一 ignored 是原有的 `todo_identifier_defenses_have_own_assertions` |

## 12. 产物 sha（`ontok-propose`）

命令（`--out` 在 `--stats` 之前、不传 `--names`）：
`cargo run --bin ontok-propose -- --schema <fix-props/schema.json> --project extracted --out <W>/artifacts/proposal.json --stats <W>/artifacts/stats.json`

| 产物 | 期望 | 实测 | 判定 |
|---|---|---|---|
| stats.json | cda6a9a2…2cd7ba | cda6a9a2…2cd7ba | ✅ 逐字节吻合 |
| proposal.json | 8dbd32db…b1db3e4c3e | 8dbd32db…b1db3e4c3e | ✅ 逐字节吻合 |

⚠️ `fix-props/schema.json` **只读**，未改动。

## 13. Dev 自述失误逐条核实

| Dev 自述 | 核实 |
|---|---|
| (a) 一对互相矛盾的自检断言 | ✅ 已修。当前 6 条「应命中」+ 3 条「不该命中」无矛盾，基线全量实跑该测试通过 |
| (b) G-3 漏写 `#[test]` | ✅ 已修。`test_anchor_guard.rs:176` 有 `#[test]`；守卫二进制实跑 **running 3 tests**，三条全 ok |
| (c) 上界断言写成会返回较近匹配的错误闭包 | ✅ 已修。`:132-136` 断言 limit=8 够得着 `fake_root`，`:138-142` 断言 limit=3 够不着（返回 None），次序正确 |
| (d) 误信只换首句的伪 M2 | ✅ **属实且判断正确**。我独立复现：该变体 **3 passed / 0 failed 存活**，但抬头仍保留「【失败类型】**二进制与当前目录不匹配**」与判别短语，两类仍可区分 ⇒ **等价变异，不是判据漏网**。用**完整替换**重跑 M2 确实被杀 |
| (e) 一次 commit session ID 打错、amend 修正 | ✅ 四个 commit 的 session ID **完全一致** `[session: ba-archlinux-80180-20261007-231834]`，无残留错值 |

## 14. 对流程偏离（Dev 改了 demand 分支）的独立看法

事实：`eed0c13` 6 个文件、全部是 `demands/*/acceptance.md` + `dispatch/req-registry.md`，
**未碰任何业务代码**；`git merge-base --is-ancestor eed0c13 d521f79` = NO
⇒ **它没有污染本次验收的代码面**（本 worktree 基线 develop 不含它）。

我的判断：**这次的处理是对的，但规则要写死，理由如下。**
- 支持：AC-004 是 BA 自己写的验收标准，点名要改 `BA/` 下的活文档。子 agent 不改 ⇒ AC 无法满足；
  改了但**单独立 commit + 显式报备** ⇒ 可被 `git revert eed0c13` 一键回退，成本极低。
  内容经我逐条核对，**全部是基线数字同步与登记订正**，无夹带。
- 风险：`BA/` 是 BA 的权威状态面。子 agent 写它，等于**两个 writer 写同一份状态**，
  即使这次内容对，**长期也会制造「谁改的」不可追溯**。
- ⇒ BA 写死的那条规则（**必须显式声明偏离 + 单独立 commit，否则一律 revert**）是对的，
  我支持，并且认为还应补一条：**偏离的 commit 不应自动进入 develop**，
  应由 BA 复核后单独合并，否则「报备」会退化成「先斩后奏」。

## 15. 发现但未修的问题（按严重度）

### 🟠 中 · M1b 存活：G-2 只断言「标签与次序」，不校验取值（已实测）
`crate_root_from_cwd()` 改成 `Some(PathBuf::from(BUILD_MANIFEST_DIR))`
⇒ 标签仍是 `Cwd`、下标仍是 0 ⇒ **G-2 三层全过**，功能上**完全还原 REQ-046**。
详见 §8。严重度定「中」是因为它是**刻意混淆**的改法，自然改法（删分支）已被 M1 证实能杀；
但它「看起来对、其实错」，比显式还原更隐蔽。

### 🟡 低 · G-1 有 4 条实测漏网
`std::env::var(...)`、`use` 别名后 `env::var(...)`、
`env!(concat!("CARGO_MANIFEST_","DIR"))`、实参离 `env!(` 超过 `ARG_WINDOW=40`。
根因：匹配锚点是 `env!(` 五字符 + 40 字窗口内找**连续子串**。
均属刻意写法，非「顺手写」；但 `std::env::var` 一条在文档里被**声称覆盖**（见下）。

### 🟡 低 · 文档过度声称（非阻断）
`test_anchor_guard.rs:480-483` 写「裸 grep 会被三样东西绕过：2. `std::env::var(...)` /
`option_env!(...)`」—— 实测 **`option_env!` 覆盖、`std::env::var` 不覆盖**。

### 🟡 低 · 「朴素 grep 8 处」在交付树上不可复现
当前树 `grep -rn CARGO_MANIFEST_DIR tests/` = **23 处**（e0f6553 与 c98a631 各 11 处）。
「8」应是开发中途的数字。但**结论方向仍成立**：剥注释后真正构成 `env!(` 命中的**代码行只有 1 处**
（`source_root.rs:104` 锚点行）。⇒ 属**叙述数字过期**，不是判据缺陷。

### 🟡 低 · F-5 引用行号差 1 行
`source_root.rs:85-86` 写 `:264/:273`，实测为 **`:265`/`:274`**。
该文档已自披露「行号会随编辑漂移，以代码为准」⇒ 非阻断。

## 16. 我自己引入又清掉的问题（如实报）

| # | 问题 | 处置 |
|---|---|---|
| 1 | 首轮全量 **125 failed / 退出码 101**，与 Dev 自述 0 失败矛盾 | **二分判定为纯环境类**：`MissingEnv("ONTOK_TEST_DATABASE_URL")`（113 次集中在 `tests/common/mod.rs:72:50`）。从仓库根 `.env` 注入、库名 `notol`→`ontol` 后重跑 ⇒ **539/0/1，退出码 0**。**125 未被用来冒充任何「杀死」** |
| 2 | G-1 探针首版 P4 `env!("X", 1)`、P11 未定义标识符 ⇒ **编译失败** | 按纪律「编译失败不算变异执行」，改写为可编译的等效构造（`(env!("X"), 1)`、`option_env!(concat!(…))`），重跑；探针自身 **16 passed** 后才取判据结论 |
| 3 | 变异后遗留 `*.bak` 未跟踪文件 | `mavis-trash` 清除；最终 `git status --porcelain` **仅 `?? .review-workspace/`** |
| 4 | 一次 shell 手误（多余反引号）导致命令报错 | 立即重发，无副作用 |
| 5 | 模板比对脚本第一版因收尾 `"` vs `\` 误报「不相同」 | 改为按 Rust 字面量规则求值后比对 ⇒ **A/B 抬头 byte-identical = True** |

## 17. 结论

**PASS（带非阻断项）**

- 核心要求成立：**M1 从「536/0/1、退出码 0 全绿存活」变成「538/1/1、退出码 101、仅
  `test_anchor_guard` 一个二进制失败」** —— 全量级证据成立。
- M2 完整替换被 G-3 杀；新增裸调用被 G-1 杀；「判据恒空」防线独立验证有效。
- 报错文本 byte-identical、G-3 零 `set_current_dir`、基线同步 539、历史 report 零改动、
  红线全过、两份产物 sha 逐字节吻合。
- 非阻断项：M1b（🟠 中）、G-1 四条漏网与文档过度声称（🟡 低）、
  「8 处」叙述过期（🟡 低）、F-5 行号差 1（🟡 低）。
