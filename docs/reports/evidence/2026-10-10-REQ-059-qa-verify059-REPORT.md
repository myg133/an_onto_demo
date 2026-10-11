# REQ-059 验收报告（QA 独立复核）

- 审查对象：`verify-REQ-059` worktree，分支 `verify/REQ-059`，HEAD `f90d537`
- 基线：`80464a9`（develop，本次改动的父提交）
- 改动面：5 文件 +819/−78（`src/agent/factory.rs`、`src/bin/ontok-propose.rs`、`tests/test_agent_orchestration.rs`、`tests/test_extraction_agent.rs`、新增 `tests/test_extracted_by_provenance.rs`）
- DB 环境变量口径：`ONTOK_TEST_DATABASE_URL` 取自仓库根 `.env`，末尾 `/notol` → `/ontol`（长度 59，尾段 `ontol`）
- 全程未使用 `CARGO_TARGET_DIR` / `--release` / `ln -s` / 另建 crate；未与另一全量测试并发

---

## 结论：**PASS（带非阻断项）**

Dev 第四次对 BA 验收标准的纠正**成立**，且实测比 BA 原文更严重；
四条 AC 全部达成，四条红线零改动，全量 619/0/1，产物 sha 独立复现一致。
遗留若干**非阻断**局限，其中「闸二在 inproc 路径上仍结构上不可达」是必须让 BA 知道的规约风险。

---

## 1. Dev 说「BA 的 AC 定位错了」成立吗？—— 成立，且比 BA 写的更严重

### 1.1 基线实测（独立执行，非采信 Dev 自述）
二进制 `ontok-propose-BASELINE-80464a9`（`cargo build --bin ontok-propose`，dev profile，src 回到 80464a9 后编译）。
输入 `.review-workspace/fix-props/schema.json`，不传 `--names`（走仓内默认 `src/extract/table_names.json`，
其 `agent` = `"mcode-agent (MiniMax Code)"`、`version` = `"1.2.0"`）。

| 用例 | 退出码 | stderr 字节 | 产物 | `extracted_by.agent` | `extracted_by.version` |
|---|---|---|---|---|---|
| 默认（无参数） | 0 | 0 | 出 | `mcode-agent` | `1.2.0` |
| `--agent 虚构名` | **0** | **0** | **出** | **`虚构名`** | `1.2.0` |
| `--agent-version 9.9.9` | **0** | **0** | **出** | `mcode-agent` | **`9.9.9`** |

BA 原文写「只 `eprintln` 警告、没报错」，**暗示存在告警**。实测是 **stderr 0 字节（零告警）**，
且编造值直接进了产物。⇒ Dev 的「定位错了」成立；真实病灶比 BA 描述的更重：
不是「警告没升级成报错」，而是**那条告警分支从未被走过**，且假名畅通无阻地进了产物。

### 1.2 构造性证明（读三处代码 + 纯函数性）
1. `src/agent/factory.rs:97-100`（基线）—— `build_agent` 的 inproc 分支：
   `RuleBasedAgent::new(AgentIdentity::from_registration(registration, round), names)`
2. `src/agent/rule_based.rs:73-75` 原样存下；`:146-148` `identity()` 返回 `&self.identity`
3. `src/bin/ontok-propose.rs`（基线）—— 调用点：
   `identity_cross_check(&AgentIdentity::from_registration(&registration, round), agent.identity())`

`src/agent/registry.rs:111-117` 的 `from_registration` 是**纯函数**（只 clone 两个 String、取 `round.max(1)`，
无随机/无 I/O/无状态读）。`build_agent(&registration, …)` 收的是不可变引用，CLI 在两处之间未改写 `registration`。

设 `X = from_registration(&registration, round)`：实参「登记侧」= `X`，实参「握手侧」= `agent.identity()` = `X`。
而 `src/agent/factory.rs:392-398` 的判据是
`registered.agent == handshake.agent && registered.version == handshake.version → None`，
要返回 `Some(warn)` **必须**两字段之一不等 —— 而它们恒等。⇒ **恒返回 `None`，结构上不可达**。

### 1.3 Dev 的修法是不是唯一正确方向？—— 实质上是
决定性理由：**登记值同时喂给 `record_round`（轮次台账）与 `extracted_by`（产物元数据）**。
只改产物侧（把 `extracted_by` 覆写成别的东西）会让台账与产物**记两个不同的名字** ——
那正是 REQ-059 判定为致命的那种不一致。改上游「登记进去的就是真实执行者」让两侧**同时**变真，
这是唯一自洽的方向。

被排除的替代方案：`RuleBasedAgent::identity()` 改为自报常量而登记仍由调用方填
⇒ 两侧必然不等 ⇒ 闸二在**每一条**默认路径上都触发 ⇒ 默认路径全崩。
即该方向单独用不可行，必须连登记一起改 —— 那就是 Dev 的方案。

### 1.4 ⚠️ 必须让 BA 知道：修完之后闸二**依然**不可达
改完后两侧**仍然**都是 `from_registration(&registration, round)`（只是这个值现在是真的）。
⇒ `identity_cross_check` 在 inproc 路径上**依旧恒为 `None`**。
Dev 在 `src/agent/factory.rs` 文档里**主动写明了**：「核对虽仍无差异可报，但它比的两边不再是同一个凭空值」——
诚实且准确。真正兜住 AC-002 的是**新增的闸一 `require_declared_matches_actual`**，
它比的是「命令行声明（人）」对「`agent.identity()`（代码）」，两个来源**真正独立**。

**给 BA 的规约建议**：不要再写「让 `identity_cross_check` 在默认路径上响起来」这类 AC ——
不起真 ACP 子进程**无法实现**。要判「核对有效」只能对 acp 路径做（Dev 已列为未修项 ②）。

---

## 2. 修完可信吗？—— 可信

### 2.1 AC-001/002 判别输入（HEAD 二进制实跑）
| 用例 | 退出码 | 产物 | stderr 首行 |
|---|---|---|---|
| 默认 | 0 | 出 | （无） |
| `--agent 虚构名` | 1 | **无** | `拒绝出产物：--agent 声明的执行者是 "虚构名"，而这一轮真正跑起来的实现是 "inproc:ontok::agent::RuleBasedAgent"` |
| `--agent-version 9.9.9` | 1 | **无** | `拒绝出产物：--agent-version 声明的执行者是 "9.9.9"，而这一轮真正跑起来的实现是 "0.1.0"` |
| `--agent inproc:… --agent-version 0.1.0`（相符） | 0 | 出 | （无） |

产物逐字取值：`extracted_by.agent = "inproc:ontok::agent::RuleBasedAgent"`、
`extracted_by.version = "0.1.0"`。stdout 两行：
```
翻译表来源   : agent=mcode-agent (MiniMax Code) v1.2.0（知识资产：**谁产的这份翻译表**，不是抽取执行者）
抽取执行者   : agent=inproc:ontok::agent::RuleBasedAgent v0.1.0（协议 inproc）—— **这一行才是 extracted_by 记的**
```

### 2.2 独立构造的绕过尝试（全部被拒）
`--agent` 闸是**严格相等**：

| 尝试 | 退出码 | 产物 |
|---|---|---|
| 完全相符值 | 0 | 有 |
| 尾随空格 `inproc:… ` | 1 | 无 |
| 全大写 `INPROC:ONTOK::AGENT::RULEBASEDAGENT` | 1 | 无 |
| 加后缀 `… (MiniMax Code)` | 1 | 无 |
| 只给 `inproc` | 1 | 无 |
| 去掉协议前缀 `ontok::agent::RuleBasedAgent` | 1 | 无 |
| 版本：`0.1.0 ` / `v0.1.0` / `0.1.0-beta` / 空串 | 1 | 无 |

⇒ AC-002 的闸一**无绕过**。

### 2.3 产物深比较（基线 vs HEAD，整份 `diff`）
`diff base-default/proposal.json head-default/proposal.json` ⇒ **仅 4 行差异**（2 删 2 增），全在 `extracted_by`：
`"agent": "mcode-agent"` → `"inproc:ontok::agent::RuleBasedAgent"`；`"version": "1.2.0"` → `"0.1.0"`。
`items` 基线 171 条 / HEAD 171 条；`stats.json` 用 `cmp` 比对 **IDENTICAL**。

---

## 3. AC-003「读起来像同一个」判据可靠吗？—— 可靠，但它是启发式而非认证

判据（`src/agent/factory.rs` 的 `executor_name_collision`）：归一化 = `trim().to_ascii_lowercase()`，
命中 = `a==b` 或 `a.starts_with(b)` 或 `b.starts_with(b)`，短侧长度 <4 豁免。**实跑 11 种形状**：

| 翻译表产出方名 | 退出码 | 判据 | 说明 |
|---|---|---|---|
| `inproc:ontok::agent::RuleBasedAgent` | 1 | ✅ | 完全同名 |
| `inproc` | 1 | ✅ | 产出方是执行者前缀（**实测出事的那种**） |
| `inproc:… (MiniMax Code)` | 1 | ✅ | 执行者是产出方前缀 |
| `INPROC:…` | 1 | ✅ | 大小写 |
| `inproc `（尾空格） | 1 | ✅ | 首尾空白 |
| `ontok::agent::RuleBasedAgent` | **0** | ❌ **放过** | 去掉 `inproc:` 协议前缀，读起来仍像同一个 |
| `in-proc:…` | **0** | ❌ **放过** | 连字符替冒号 |
| `inproc:…::Rule_BasedAgent` | **0** | ❌ **放过** | 下划线替驼峰 |
| `in` | **0** | 放过（**有意**） | 短名豁免，代码注释已写明取舍 |
| `mcode-agent (MiniMax Code)`（仓内真实表） | 0 | ✅ 正确放行 | 不撞名，生产路径必须能跑 |
| `table-name-translator` | 0 | ✅ 正确放行 | 对照组 |

**Dev 的论证成立**：只判「完全相等」的判据在实测案例（`mcode-agent (MiniMax Code)` vs `mcode-agent`）上**不会响** ——
两个字符串确实不相等。M3 变异（删掉两个 `starts_with` 分支）被两条 AC-003 用例杀死，死因正是前缀形状。

**残留局限（须登记）**：判据能挡住「前缀」「大小写」「首尾空白」，挡不住「非前缀但形近」。
它是**抬高门槛、降低误读**的启发式，**不能防冒充**。这是相对基线（基线连判据都没有）的**净改善**，非回归。

stdout 两行**确实消歧**：一行讲知识资产、一行讲执行者，且明写「这一行才是 extracted_by 记的」。

---

## 4. 4 条既有测试的改动 —— **接受**（含 1 条轻微建议）

### 4.1 原前提确实是缺陷（不是仍然成立）—— 双向实证
- **基线侧**：基线全量 610/0/1，4 条原始断言**全部 `ok`** ⇒ 「默认路径 ≡ registry 路径」当时**确实成立**。
- **HEAD 侧**：改后两条 registry 测试里的 `assert_ne!` 在 619/0/1 中**通过**
  ⇒ 两条路径的 `extracted_by.agent` 现在**确实不同**。

所以「相等」这个前提被修复**必然打破**，收窄是**被逼的**，不是挑肥拣瘦。
根因也如 Dev 所述：相等只因登记时照着占位抄了 —— 占位一去掉，相等就消失。

### 4.2 逐条：守什么 / 放过什么
| 测试 | 改动 | 仍在守 | 放过 |
|---|---|---|---|
| `test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | 整份逐字节 → 分字段 | `stats.json` **整份逐字节**；`items`/`round`/`extracted_by.source`/`failed_groups` 相等；**并新增** agent/version 必须等于两个常量 | 「整份 proposal 逐字节相同」（已不成立） |
| `test_cli_default_and_registry_paths_remain_byte_identical` | 同上 | 两路径 `items`/`round`/`source`/`failed_groups` 相等；**并新增** 默认路径=常量、registry 路径=`mcode-agent`、两条 `assert_ne!` | 整份逐字节相同；**未再断言两路径 `extracted_by.round` 相等**（轻微） |
| `test_cli_registry_path_is_byte_identical_when_registry_says_the_same` | 同上 | 同上（另保留原有 round=3 子断言） | 同上 |
| `test_the_production_entry_still_reports_the_identity_fork` | 「必须打出来」→「必须拦住」 | 仍调 `identity_cross_check(`；调用点 600 **字节**窗口内必须 `bail!(`；全文不得再出现 `⚠ {warn}`；`build_agent` 必须在调用点之前 | 「告警必须被打印出来」（已被 REQ-059 推翻） |

**是否收窄过头？—— 没有。** 四条都**新增了正向断言**（钉住具体常量值 / `assert_ne!`），
净效果是对「归属」这一维度**更严**，只对「内容」这一维度保持原强度。

### 4.3 我对该源码断言做了独立强度验证（并推翻了自己的怀疑）
我一度怀疑第 4 条**不严**：因为闸三的 `anyhow::bail!` 也落在同一窗口内，可能顶替闸二的 bail。
**按码点切片测量时看起来确实如此**（闸三 bail 在「偏移 504 < 600」）。
但 Rust `str` 切片按**字节**，而文件含大量 3 字节 CJK —— **按字节重测：闸三 bail 在偏移 622，已在 600 字节窗口之外**。
⇒ 构造 **M4**（把闸二降级为只告警、保留闸三 bail）实跑：该测试**变红** ⇒ 断言**有效**，我的怀疑不成立。

### 4.4 轻微建议（非阻断）
- **函数名与语义相反**：`test_the_production_entry_still_reports_the_identity_fork` 现在断言的是
  **拦截（refuses）**而非「报出（reports）」，名字已误导。建议改名，例
  `test_the_production_entry_refuses_when_the_identity_fork`。
- 窗口 600 是魔数；闸三 bail 距窗口边缘仅 22 字节，后续重构易误伤（**偏严方向**，安全）。

### 4.5 流程规约裁决（回答「改既有测试的前提该由谁定」）
- **Dev 主动上报 + 给出可验证理由 + 本次收窄被独立实证为「被修复必然打破」** ⇒ 本次**接受**，且应记为正面。
- 但**规约应明确**：改既有测试的前提**不是 Dev 单方决定权，也不是 QA 单方裁量**。
  建议固定为三步：①Dev 改动时**必须**在同一处写明「原前提 + 为何失效 + 收窄后仍守什么」；
  ②QA **独立复现原前提在基线成立、在 HEAD 不成立**（本次已做）；
  ③**前提被推翻这一事实**由 BA 认领并写回 acceptance.md ——
  本次 BA 尚未在 acceptance.md 里记「原逐字节一致判据作废及其原因」，**建议补记**（见 §8）。

---

## 5. Dev 主动报的五处失误 —— 全部核实为真修

1. **假通过（最要紧）**：核实 `src/bin/ontok-propose.rs` 全文已无字面量 `⚠ {warn}`（`grep` 退出码 1）；
   断言改成 `!src.contains("⚠ {warn}")` ⇒ 注释里复述该字面量会**真的变红**。「散文里写着 ≠ 代码里还在」已修。
   现存两个 `eprintln!`（`:428` 抽取失败、`:433` 映射核不过去）均与身份告警无关。
2. **测试互相污染**：`scratch()` 用 `{tag}-{pid}-{thread}`，各用例 tag 互异；
   实测目录形如 `ac003-cli-12-…-ThreadId(2)` 与 `ac003-cli-36-…-ThreadId(2)`（同线程不同 tag）⇒ 不再互相覆盖。
3. **参数名报错**：实测 stderr 为 `拒绝出产物：--agent-version 声明的执行者是 "9.9.9"…` ⇒ 指名正确。
4. **变量遮蔽**：`declared_agent`/`declared_version`（`:72-73`），`let agent = build_agent(..)` 在 `:221` ⇒ 不再遮蔽。
5. **`skip_serializing_if`**：`src/extract/propose.rs:99` 确认 `source` 带该属性；
   类型形状测试给 `source: Some(..)`（`:351`）⇒ 5 个键齐全，断言不失真。

---

## 6. 测试 / 红线 / 产物 sha

### 6.1 全量（口径：日志中全部 `test result:` 行，`awk` 累加 `$4/$6/$8`；不用 `paste -sd+`）
| | `test result:` 行数 | 通过 | 失败 | 忽略 |
|---|---|---|---|---|
| 基线 `80464a9` | 44 | **610** | 0 | 1 |
| HEAD `f90d537` | 45 | **619** | 0 | 1 |

差值 **+9**，与新增文件 `tests/test_extracted_by_provenance.rs` 的
`grep -c '^#\[test\]'` = **9** 一致；多出的 1 行是该新测试二进制。

### 6.2 无删除 / 无 `#[ignore]`
- `#[ignore]`：基线 1 条、HEAD 1 条（`git grep -hE '^\#\[ignore' 80464a9 -- 'tests/*.rs' | wc -l` 对比）。
- 测试函数名集合差：基线 387 个唯一名、HEAD 396 个；`comm -23`（基线有 HEAD 无）**输出为空** ⇒ **无测试被删除**；
  `comm -13` 恰为 9 个新增函数名。

### 6.3 四条红线
`git diff --numstat 80464a9 HEAD -- src/domain/view.rs src/domain/review.rs tests/drop_rate.rs tests/test_fk_naming.rs`
⇒ **无输出（各 0 行）**。
`test_humanize_still_refuses_unknown_columns` 位于 `src/domain/view.rs`，该文件 **0 行改动** ⇒ 一条断言都没放宽。
收尾以 `sha256sum -c` 复核四文件与进入时记录**逐一一致**。

### 6.4 产物 sha（独立复现，两次：初次 + 全部变异复原后重建）
- `stats.json` = `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` ⇒ **未变**，与任务书一致。
- `proposal.json` = `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` ⇒ 与**新**基线一致。
- 同时独立复现了**旧**基线 `8dbd32db4f97e897152cda7b580b6c382f320d2506ad3f59642f2fb1db3e4c3e`
  （在 baseline 二进制上跑出）⇒ 确认「基线已变、新旧对应」无误。

### 6.5 新基线 sha 的文档同步 —— **无漏改**
范围 `/home/myg133/Projects/Anchnet/Ontology/BA`（分支 `demand`）。
含新 sha 的文件 **46 份**；仍含旧 sha 的 **7 份**，逐条判定**全部为正当历史/前后对照**：
`dispatch/req-registry.md`（前后对照）、`REQ-059-dev-FINDINGS.md`（基线实测＋对照）、
`REQ-043`/`REQ-042`（实测列**已是新 sha**）、`REQ-047`/`REQ-050`（2026-10-09 时点验收记录，改之等于篡改历史）、
`REQ-034`（历史说明）。
**acceptance.md 全量 25 份核查**：21 份含新 sha；余 4 份（REQ-001-spike / 002-kernel-mvp / 041 / 048）
经 `grep -c 'proposal\.json'` 与 `grep -cE '[0-9a-f]{64}'` 核实**均为 0 处**，即从未记录过 sha ⇒ **不是漏改**。

---

## 7. 变异测试表

**执行口径**：`cargo test --test test_extracted_by_provenance --test test_agent_orchestration
--test test_extraction_agent --no-fail-fast`（三个相关二进制）。
**编译失败不计为变异执行**（本轮无编译失败）。每次变异前记录 sha256，跑完 `git checkout` 复原并 `sha256sum -c` 校验。

| 变异 | 内容 | 结果 | 红了几条 | 杀死它的判据（用例名） |
|---|---|---|---|---|
| **M1** | 默认登记改回硬编码占位（`"mcode-agent"` / `"1.2.0"`） | **杀** | **8** | 新文件 5 条：`test_default_path_records_the_real_executor_not_a_placeholder`、`test_declared_agent_that_matches_the_executor_is_recorded_verbatim`、`test_fabricated_agent_name_with_inproc_must_be_rejected`、`test_provenance_change_does_not_alter_the_extracted_content`、`test_cli_refuses_to_emit_when_executor_name_collides_with_table_producer`；另 3 条既有测试：`test_cli_end_to_end_output_is_byte_identical_to_legacy_extract`、`test_cli_default_and_registry_paths_remain_byte_identical`、`test_cli_registry_path_is_byte_identical_when_registry_says_the_same` |
| **M2'** | `require_declared_matches_actual` 的 `bail!` 降级为 `eprintln` + `return Ok(())` | **杀** | **2** | `test_fabricated_agent_name_with_inproc_must_be_rejected`、`test_fabricated_agent_version_must_be_rejected` |
| **M3** | `executor_name_collision` 删掉两个 `starts_with` 分支，只留 `a == b` | **杀** | **2** | `test_translation_table_and_executor_names_must_be_distinguishable`、`test_cli_refuses_to_emit_when_executor_name_collides_with_table_producer`（死因均为**前缀**形状） |
| **M4**（本轮新增，不在 Dev 自述内） | 闸二 `identity_cross_check` 的 `bail!` 降级为 `eprintln`，保留闸三 `bail!` | **杀** | **1** | `test_the_production_entry_still_reports_the_identity_fork` |

### 存活变异
**无。三条 Dev 自述变异 + 本轮新增 M4 全部被杀死。**

### 与 Dev 自述的差异（据实登记）
- **M1**：Dev 报「5 条红」＝ 新测试文件内的 5 条；我在**三个二进制**口径下测得 **8** 条。
  两个数字各自成立，**口径不同**（Dev 未说明其口径）。
- **M2'**：Dev 报「4 条红」，我在同一口径下只测得 **2** 条，**未能复现 4**。
  已排查：`tests/test_extraction_agent.rs:1416` 的 `test_cli_rejects_registry_plus_explicit_agent`
  虽用 `--agent`，但它在**更早**的「registry 与显式来源并存」检查处就 bail，M2' 触不到它，
  实跑确为绿（该二进制 26 passed / 0 failed）。差异原因**未能定位**，不影响结论（M2' 已被决定性杀死）。

---

## 8. 发现但未修的问题（均非阻断）

1. **闸二在 inproc 路径上仍结构上不可达**（§1.4）。Dev 已主动登记为未修项 ②。**影响**：若将来有人把闸二当作
   「身份核对已生效」的证据，会被误导。**建议**：BA 在 acceptance.md 记明该判据当前只对 acp 路径有意义。
2. **AC-003 判据的非前缀形近缺口**（§3）：`ontok::agent::RuleBasedAgent`、`in-proc:…`、`Rule_BasedAgent` 可通过。
   属启发式固有局限，非回归。
3. **`INPROC_EXECUTOR_ID` 是硬编码字符串**，重复了类型路径。若将来重命名/移动 `RuleBasedAgent`，
   该常量会**静默过期**。更自维护的写法是 `std::any::type_name::<RuleBasedAgent>()`（代价：不能是 `const`）。
   **改进建议，非缺陷**（现值正确且被测试钉住）。
4. **测试函数名与语义相反**（§4.4）：`…still_reports_the_identity_fork` 实为「必须拦住」。
5. **Dev 已登记的未修项 ①**：**没有 CLI 能登记 agent**。`--agent` 变为「断言」后，
   想用自定义名字的用户**没有受支持路径**，只能直接写库 `PgAgentRegistry::register`。
   这是**真实可用性缺口**，本需求引入，建议单独立条。
6. **registry 路径 + 仓内真实翻译表会撞闸三（分析推断，未实测）**：
   若 registry 登记名为 `mcode-agent`（既有两条测试正是这么登记的）而翻译表产出方为
   `mcode-agent (MiniMax Code)`，则闸三会 `bail`。既有两条测试用的是 `table-name-translator`，故不受影响；
   但**生产上**「用仓内真实表 + 登记名 `mcode-agent`」这条组合的行为变更**不在任何 AC 覆盖内**，
   也无测试。**未实测**原因：需在共享本体库写入 agent 登记，本会话纪律禁止写共享库。
   **建议**：BA 明确该组合是否应当被拒。

---

## 9. 我自己引入又清掉的问题（如实登记）

1. **误判一次**：我先用 Python **按码点**切 600 字符窗口，得出「闸三的 bail 落在窗口内 ⇒ 该源码断言不严」的
   **错误结论**。发现原因是 Rust `str` 切片按**字节**而文件含 3 字节 CJK。改按字节重测（闸三 bail 在偏移 622）
   并构造 **M4** 实跑证伪 ⇒ 该断言**有效**。已写入 §4.3。此为本次最容易造成误报的一处，特此登记。
2. **shell 分词事故**：早期一批绕过测试里 `虚构名`/`9.9.9` 因 `${CASE#*:}`+`cut` 组合被吃掉，
   实际测的是空串。已用不带循环技巧的命令重跑，上表所有数字均来自重跑结果。
3. **变异期间的临时改动**：M1/M2'/M4 改过 `src/bin/ontok-propose.rs`，M3 改过 `src/agent/factory.rs`，
   每次跑完立即 `git checkout` 复原并 `sha256sum -c` 校验通过。
4. **基线对照期间**曾把新增测试文件 `tests/test_extracted_by_provenance.rs` 移出 `tests/`，跑完已移回。

## 10. 收尾状态
- `git status -uall` **空**；HEAD = `f90d537`
- 四条红线 `sha256sum -c` **全部成功**
- `tests/` 下无遗留临时文件
- `.review-workspace/fix-props/schema.json`：`mtime = 2026-10-07 23:55:23`（**早于本会话起始 2026-10-10 11:06**）
  ⇒ 全程只读、未被改动
- 临时文件只落在 `.review-workspace/verify-req059/`；未使用 `/tmp` 或 home
