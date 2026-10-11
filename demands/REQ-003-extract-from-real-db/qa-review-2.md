# REQ-003 修复轮复审报告

## 0. 审核元信息

| 项 | 值 |
| --- | --- |
| 审核范围 | `feature-REQ-003` @ `75bd985`（代码）· `BA` @ `4fa379b`（脚本） |
| 首轮报告 | `~/req003-review/REVIEW.md`（659 行 · VERDICT 不通过 · 2 阻断 + 1 严重） |
| 环境 | cargo/rustc 1.98.1 · 本体库 = 外部 PG(非标准端口 30432 代理) · 只读，未做任何 DELETE/TRUNCATE |
| 基线 | 独立跑出 **160 测试全绿**（lib 22 + 10 个集成套件）· 148 基线 + 12 新增，数目与声称一致 |
| 变异方法 | 每变异独立 target 目录（**真实拷贝，非 hardlink**）；还原后显式 `touch` 强制重编（破 mtime 陷阱） |
| 证据台账 | `~/req003-review2/logs/evidence.md`（123 行）· `logs/flake2.log` · `logs/revert-r4.log 与 logs/revert-r5.log` |
| **一句话结论** | **B1、B2 真修好了（实测可证）；S1 的三个指定变异真被抓住，但接线判据可被「诱饵调用」绕过 —— 判据有数、有鉴别力，但判不出「守卫是否真的在强制」。无阻断项，1 个严重项建议同批修。** |

### 9 条自述裁定表

| # | 自述 | 裁定 | 证据 |
| --- | --- | --- | --- |
| 1 | `cardinality_cn` 补 `"N:1" => "多对一"`，与既有风格同构 | **属实** | `src/domain/view.rs:286`，与 282-284 同构。真实产物 `out/e2e-view.json` 实测 `{'多对一': 51}`（修复前 `{'N:1': 51}`） |
| 2 | 不加 `n_1` 到词表，改用形态判定 `cardinality_notation_hit`（「词表是枚举，形态是判定」） | **属实** | `view.rs:436-440` 词表确无 `n_1`；`view.rs:461-482` 形态判定。探针实测必抓 14/14（含词表覆盖不到的 `1:M`/`M:N`/`1..*`/`*..*`/`x:1`）→ 分界是真的 |
| 3 | B1 变异自证：删 `N:1` 分支 → 2 条红；抽掉第三档 → 另外 2 条红；两组互不遮蔽 | **属实** | M1 红在 `test_cardinality_cn_translates_every_known_notation` + `test_view_translates_every_known_cardinality_notation`；M2/M3 红在 `test_guard_catches_any_cardinality_notation` + `test_view_no_cardinality_notation_passes_silently`。四组互不遮蔽，已逐一复现 |
| 4 | B2：判据抽成 `checks_step2()`/`checks_step6()`，`chk()` 计数，`SystemExit(1)`，`FAIL=1` → `finish()` 非 0；不可解析算 fail-closed | **属实** | 四次退出码 **0 / 1 / 1 / 0**；不可解析六连（截断/空/缺文件/视图数组/视图 null）全 1，恢复 0。判据 4 个调用点全被 `if` 捕获，两处 `\|\| true` 只在 grep 取别名（良性） |
| 5 | B2 自测：正常 0 → 注入 11 处缺陷 1 → 恢复 0；加了 `REQ003_CHECKS_ONLY=1` | **部分属实** | 四连我独立复现属实；「11 处缺陷」未逐条复现（时间）。**自述未提的实测结果：判据模式对 B1 零敏感** —— 把 51 条 `how_many` 全改回 `N:1`（修复前形态）仍 **exit 0**；`meaning` 改回 `belongs_to` 也 **exit 0** |
| 6 | S1：抽成 `ensure_read_only()`/`read_only_pool()`，新增 5 条测试，**三个变异全部被抓** | **部分属实** | R1/R2/R3 三个**指定**变异确实全被抓住（已复现）。**但** R4/R5 两个诱饵变异让 **160/160 全绿**，其中 R5 是真 fail-open（详见 §3.2） |
| 7 | `verify_selftest.sh` 10→11，真在测判据的 5→9；AC-101 存在性 grep → 行为测试；AC-105 → 实跑 + 防空跑自检 | **部分属实** | 11/11 实跑通过；AC-105 防空跑自检我用两种错过滤器验证确实报出。**但** 11 条里 1 条（「勘查器源码无写操作字面量」）仍是真实文件的静态检查、无注入，判不出过滤器本身有无鉴别力；且 AC-101 只读判据与 Rust 侧同源，同样漏 R4/R5 |
| 8 | 完整 E2E 复跑：`{'多对一': 51}`、零透出、exit 0 | **属实（就产物而言）** | `out/e2e-view.json` 实测 91 对象 / 51 关系 / `{'多对一': 51}`，零透出。**完整 E2E 我无法复跑（无源库连接串）** |
| 9 | 160 测试全绿 | **属实** | 独立跑出 160，逐套件计数吻合；`git diff` 确认新增恰 12 条测试 |

**统计：属实 6 · 部分属实 3 · 不属实 0。**

### 本次实际完成的任务

**做了**：B1 的 6 个变异（含 3 个自设计绕过）· B2 四次退出码 + 6 种 fail-closed + CHECKS_ONLY 后门评估 · S1 的 5 个变异（含 2 个诱饵绕过）· 守门器 39 项直接探针 · 性能实测 · `verify_selftest.sh` 11 条逐条定性 + 2 种错过滤器 · L5 根因定性 · AC 覆盖现状。

**没做及原因**：
- **完整 E2E 实跑** —— 无源库连接串（客户提供、不入库）。CHECKS_ONLY 模式已覆盖第 2/6 步判据。
- **`--accept-all` 行为验证** —— 结构上确认未被波及：`git diff 7d12548..75bd985 --name-only` 只含 `view.rs`/`introspect.rs`/两个测试文件，`cli.rs`/`review.rs` 未触及，故不可能被本轮改变。未实跑该路径。
- **`ensure_read_only_rejects_anything_but_on` 的专门变异** —— 未实跑；该测试断言 7 个坏值必须 `is_err`，函数体若被改成恒 `Ok` 必然红（逻辑必然，未实证）。
- **12 条新测试逐条单独打变异** —— 基数相关 5 条、只读相关 5 条已覆盖；`stripper_strips_comments_before_matching`、`read_only_pool_itself_is_not_wired_to_reject_everything` 两条未单独打变异。
- **误杀面收紧方案未在真实代码上实现验证** —— 我不改被审对象；只验证了「形态档本身有鉴别力」。

---

## 1. B1 复核

### 1.1 变异结果（含 REVERT-CHECK）

| ID | 变异 | 结果 | 抓住它的测试 |
| --- | --- | --- | --- |
| M1 | 删 `cardinality_cn` 的 `"N:1"` 分支 | 🔴 RED | `view.rs::test_cardinality_cn_translates_every_known_notation` + `test_view.rs::test_view_translates_every_known_cardinality_notation` |
| M1-R | 还原 | 🟢 22+21 | `view.rs` 与 base 逐字节相同 |
| M2 | 第三档形态守门改成 `return None` | 🔴 RED | `view.rs::test_guard_catches_any_cardinality_notation` + `test_view.rs::test_view_no_cardinality_notation_passes_silently` |
| M2-R | 还原 | 🟢 22+21 | 同上 |
| M3 | **形态判定退化成只认 `N:1` 的枚举** | 🔴 RED | 同 M2 的两条（另 7 种未知拼法全漏） |
| M3-R | 还原 | 🟢 22+21 | 同上 |
| M5 | 译文里嵌回记号 `"多对一（N:1）"` | 🔴 RED | 两条 `translates_*`（精确等值断言） |
| M5-R | 还原 | 🟢 22+21 | 同上 |
| M6 | 兜底产出带空格记号 `"X : Y"`（守门器抓不到） | 🔴 RED | `test_view_no_cardinality_notation_passes_silently` |
| M6-R | 还原 | 🟢 22+21 | 同上 |

**M6 是本轮最有价值的一条**：它证明不变量测试**不是换了个形状的假断言** —— 守门器漏掉的形态，它照样抓住。

### 1.2 两条不变量测试是真不变量吗

**是。** `test_view.rs:337-371` 的判据是一个**析取式不变量**：

```
translated（按 KNOWN_CARDINALITIES 精确译对） ∨ caught（守门器抓到）
```

它**不预设守门器工作**，只要求「要么译对、要么被抓住」，与实现无关。三个证据：

1. **M1 下它保持绿**（译错但守门器抓到了 → 不变量仍成立）—— 说明它测的不是翻译表。
2. **M2/M3 下它变红**（守门器失效且没译对 → 不变量破）—— 说明它对守门器失效敏感。
3. **M6 下它变红**（守门器有盲区且没译对）—— 说明它能发现守门器的盲区。

**不会退化成空断言**：`notations` 数组若被清空会变成 vacuous pass，但那是「改测试」而非「改实现」，不在本次范围。`KNOWN_CARDINALITIES` 若被清空反而让测试**更严**（所有记号都必须被抓住）。

`test_view.rs:321-334` 是枚举式的「已知记号必须译对」，单独看确实只锁 4 种；但两条测试**互锁** —— 若有人加了 `"1:M" => "多对多"` 却没登记进 `KNOWN_CARDINALITIES`，测试②会因 `translated=false` 且守门器抓不到「多对多」而变红。

### 1.3 新发现的绕过手法

直接调真实 Rust 函数（不是 grep），39 项探针：

**必抓 14/14**：`N:1` `n:1` `1:N` `N:M` `1:1` `1:M` `M:N` `0:1` `1:0` `1..*` `0..1` `N..M` `*..*` `x:1`
→ 大小写（`n:1`）**有效**，因为 `side()` 用 `is_ascii_alphanumeric` 而非精确字符。

**🔴 漏网 10 种**（`view.rs:465-477` 的形态判定严格 ASCII + 分隔符必须紧邻）：

| 输入 | 漏网原因 |
| --- | --- |
| `N : 1` / `1 :N` / `N: 1` / `1 : 1` | 分隔符两侧有空白 → `b[i+1] != ':'` |
| `Ｎ：１` / `N：1` / `1：1` | 全角分隔符 `：` 不是 ASCII `:` |
| `N∶1` / `1∶1` | `∶`(U+2236) 不是 `:` |
| `1...1` | `..` 后紧跟 `.`，`j` 落在 `.` 上，`side('.')` 为 false |

**可达性（重要）**：`propose.rs:159-164` 的 `REL_SUFFIXES` 是**闭合字面表**，只产出 `"1:N"` / `"N:1"`。因此上述绕过**从当前产出侧不可达**，仅在「手改提案 JSON 走 `propose import`」时才可能（`apply.rs:247-248` 直接 `.get("cardinality")`，**不校验取值**）。定为**理论缺口，非现存缺陷**。

### 1.4 误杀面评估：阻断？收紧方案？风险可接受？

**实测误杀面**（探针直接调 `scan_modeling_terms`）：

| 输入 | 结果 |
| --- | --- |
| `09:30` `12:30:00` `23:59` `2026-10-03T10:41:55Z` `1.5` `v1.2.3` `http://example.com` `https://a.b:8080/x` `AA:BB:CC:DD:EE:FF` `2026-10-03 10:41:55` `10:20 会议` `唯一对应` `多对一`… | ✅ 不误杀（15 项，修复方声明准确） |
| `比例是 1:1 的概率很小` | 🔴 误杀（中文语境比例） |
| `1:2:3` | 🔴 误杀（命中 `1:2`，版本号格式） |
| `a1:2:3` | 🔴 误杀（命中 `2:3`，扫描逐字符、不锚定词边界） |
| **`第 3:1 季度`** | 🔴 误杀（**本轮新增，status.md L1 未列**） |

**真实产物零误杀**：137,969 字节 → `hits=0`（实测）。

**① 真实风险有多大 —— 中低，但非零，且会随范围扩大而升高**

- 失败模式是**安全方向**：`cli.rs:520-522` → `exit 2` 拒发整份视图。误杀 = 拒发，**不是把错数据给客户**。这是 fail-closed，方向正确。
- 触发面取决于哪些字段是自由文本。客户视图的字段里，**`note`（源库列注释，客户自己写的）与 `sample`（样例值）是自由文本**，最可能含 `1:1`／版本号／比例。
- **当前零风险**：`sample` 全部为 `null`（遵循「不发数据行」的授权边界），`note` 是中文列注释。
- **风险何时变正**：一旦按 AC-013 组织外部评审、或未来把样例值接进 `sample`，风险从 0 变正。属性名质量那 8 个待修项若引入英文兜底，也会增加命中面。

**② 低成本收紧方案（两条，都便宜）**

- **方案 B（推荐，改动最小）**：形态档只认**作为 `how_many` 字段完整值**的记号 —— 正则 `"how_many"\s*:\s*"[^"]*[:：∶][^"]*"`。语义最准：`note`/`sample` 里的记号完全不误杀，`how_many` 仍受保护。约 3 行。
  *代价*：`from`/`to` 字段里出现的记号不再受保护 —— 但那两处是中文对象名，风险极低。
- **方案 A（加强）**：保留全文扫描，但先反序列化，把 `objects[].note`、`properties[].note`、`properties[].sample`、`aliases[]` 这些自由文本字段剔除后再扫形态档。约 15 行。
  *代价*：引入解析分支；**解析失败必须退回全文扫**以保持 fail-closed。

**③ 不收紧的风险是否可接受 —— 可接受，不阻断合并**

理由：① 净收益明确（泄漏 100% → 0，且真实产物零误杀有实测支撑）② 失败模式是拒发而非错数据，安全方向正确 ③ 收紧成本低，不宜作为合并门槛 ④ 当前产物 137KB 零误杀，不存在现实触发路径。

**但建议把「收紧」列为紧跟项，并把已知误杀样例补进 `test_guard_does_not_mistake_timestamps_for_notation`** —— 那条测试目前只锁 10 个不误杀样例，上面 4 个已知误杀**一个都没进去**，意味着误杀面扩大时无人察觉。

---

## 2. B2 复核

### 2.1 四次退出码实测

脚本在 sandbox 内**逐字节复制**（sha256 与原件一致），跑在 `fakerepo/` 上，**全程未修改 `/home/myg133/Projects/Anchnet/Ontology/` 下任何文件**。

| 场景 | 退出码 |
| --- | --- |
| 正常产物 | **0** |
| 破坏提案（`project` 改错 + 实体砍到 3 条） | **1** |
| 破坏视图（`objects`/`relations` 清空） | **1** |
| 恢复（sha256 与原件逐位一致） | **0** |

**B2 是真修好了。** 首轮那个「8 项全 ✗ 而退出码仍是 0」的形态已消失。

### 2.2 CHECKS_ONLY 是不是后门

**不是绕过真实流程的后门**：`e2e-req003.sh:133-151` 在第 133 行进入判据模式，第 150 行 `finish()` 退出，**结构上不会落回**第 0-8 步的真实流程。

**但它有严重的覆盖盲区，自述没提**：

| 把真实产物改成 | CHECKS_ONLY 结果 |
| --- | --- |
| 51 条 `how_many` 全改回 `N:1`（= 修复前的 B1 形态） | **exit 0**，第 6 步 3/3 通过 |
| 51 条 `meaning` 改回 `belongs_to`（L2 语义泄漏） | **exit 0** |

根因：`checks_step6` 只判三项（对象数非空 / 平均属性数≥3 / 零属性对象==0），**从不判 `how_many`/`meaning` 是否人话**。B1 的真实防线是 `cli.rs:518-523` 的 `exit 2`，而 CHECKS_ONLY **根本不执行 `ontology show`**，所以那条防线在判据模式下不存在。

**结论：判据模式可以当「第 2/6 步量化判据是否会让脚本变红」的回归工具，但绝不能当 B1 的验收证据。** status.md 的 L4 只提了「第 2 步第 1 项自比」，**没提这个更严重的盲区**，建议补记。

### 2.3 fail-closed 是否成立

**成立，且比声称的更严。** 六种不可解析输入全部 exit 1：

| 输入 | 退出码 |
| --- | --- |
| 提案 = 截断的非法 JSON / 空文件 / 缺文件 | 1 / 1 / 1 |
| 视图 = 截断的非法 JSON / `[]` / `null` | 1 / 1 / 1 |
| 恢复 | 0 |

代码核对：`checks_step2`/`checks_step6` 的 `try/except` 走 `chk(False,…)` + `SystemExit(1)`；判据 4 个调用点全部包在 `if …; then ok; else bad; fi` 里；`bad()` 置 `FAIL=1`，`finish()` `exit $FAIL`。**没有 `|| true` 吞掉判据** —— 脚本里仅有的两处 `|| true`（第 201、245 行）是 `grep -oE … | head -1` 取提案别名的良性兜底。

一处**残留的 print-only 味道**（非阻断）：第 7 步 `ontology diff … | head -4 | sed … && ok "diff 可执行"` —— 靠 `set -o pipefail` 才没变成永真，但这也意味着 `head -4` 提前关管道可能给 `diff` 带来 SIGPIPE 而**假红**。方向是假红不是假绿，可接受。

### 2.4 AC 判据覆盖现状

| AC | 有判据测试？ | 位置与形态 | 首轮指出后是否补上 |
| --- | --- | --- | --- |
| AC-101 只读 | ✅ **有** | `test_read_only_guard.rs` 5 条 + introspect.rs 2 条 + `verify_selftest.sh` 4 注入 | ✅ 已补（但见 §3.2） |
| AC-102 勘查元数据 | ❌ **无** | 只有 E2E 第 1 步 `[ "$NTAB" -ge 80 ]`（且是 `-ge 80` 不是 `== 91`）；**无任何 Rust 测试** | ❌ 未补 |
| AC-103 提案合法 | ⚠️ 仅脚本 | `checks_step2` 9 项（已参与退出码 —— **实质改进**） | 部分 |
| AC-104 投影覆盖率 | ⚠️ **口径不符** | 见下 | ❌ 未补 |
| AC-105 守门词 | ✅ **有** | `test_view.rs` 4 条 + `verify_selftest.sh` 实跑 + 防空跑自检 | ✅ 已补 |
| AC-106 E2E | ⚠️ 仅脚本 | 第 7/8 步，判据已参与退出码 | 部分 |
| AC-107 可复现性 | ⚠️ 仅部分 | `checks_step2` 的 `extracted_by` 版本/来源 2 项；**「同一 schema 抽两次」E2E 没做** | ❌ 未补 |

**首轮「只有 AC-002/101/105 有判据测试」这条，本轮只补了 AC-101 与 AC-105；AC-102/103/104/106/107 仍无 Rust 判据测试**（103/106/107 的量化项进了 E2E 且已参与退出码，是实质改进）。

**🆕 AC-104 判据口径与 demand.md 不一致**：`demand.md:104/108` 要求判定「属性投影覆盖率 **< 60% 判 ✗**」；`checks_step6` 实际判的是「平均属性数/对象 **≥ 3**」，**从未计算覆盖率**。实测真实覆盖率 = 793/964 = **82.3%**（目标 ≥60%，实际达标），且 `stats.json` 里 `props_dropped`/`props_via_wordlist`/`props_via_comment` 字段齐全，**补这条判据约 3 行**。所以这不是质量问题，是**判据口径被替换**的可追溯性缺陷。

---

## 3. S1 复核

### 3.1 三个变异复现

| ID | 变异 | 结果 | 抓住它的测试 |
| --- | --- | --- | --- |
| R1 | 删 `after_connect` 里的 `SET` | 🔴 RED **2 条** | `read_only_pool_really_reads_back_on`（**行为层**：读回 `off` vs `on`）+ `after_connect_read_only_set_is_in_executable_code`（结构） |
| R2 | 删 `ensure_read_only` 调用点 | 🔴 RED **1 条** | `fail_closed_check_is_actually_called_by_introspect`（**仅结构**） |
| R3 | 删 `force_read_only` 调用点 | 🔴 RED **1 条** | `force_read_only_is_actually_called_by_the_pool_builder`（**仅结构**） |

REVERT-CHECK：r1/r2/r3 还原后 `introspect.rs` 与 base **sha256 逐字节一致**（`7508a7378bba`，target 目录已回收故未重跑）；r4/r5 还原后各 **160 passed / 0 failed**。

**修复方自曝的「第一版被文件内单元测试顶包」确实被杜绝了** —— `code()`（`test_read_only_guard.rs:91-103`）先剥注释再砍掉 `#[cfg(test)]` 之后的部分，并且有**锚点自检**（`production.contains("pub async fn introspect")`）防止砍错地方静默放行。这是本轮做得最扎实的一处。

**另一个正面证据**：抽掉 `ONTOK_TEST_DATABASE_URL` 时 `read_only_pool_really_reads_back_on` 直接 panic
`缺 ONTOK_TEST_DATABASE_URL … MissingEnv(...)`（`introspect.rs:360`）—— 缺环境是 **fail-closed**，不是静默 skip。

### 3.2 新绕过尝试 —— 🔴 找到，两个都骗过了 160 个测试

| ID | 变异 | 结果 |
| --- | --- | --- |
| **R4** | 池构造里删掉真 `let safe_url = force_read_only(url)?;`，改在 `introspect` 里补一次**看起来像防御纵深**的 `let _ = force_read_only(url)?;` | 🔴 **160/160 全绿** |
| **R5** | 删掉 `ensure_read_only(&read_only)?;`，补一次 `ensure_read_only(&read_only).ok();`（**错误被丢弃 = 真 fail-open**） | 🔴 **160/160 全绿** |

**根因**：`test_read_only_guard.rs:147` 与 `:159` 的判据是

```rust
let calls = c.matches("force_read_only(").count();
assert!(calls >= 2, …);      // 同 ensure_read_only(
```

**数出现次数，判不出调用是否在正确位置、也判不出错误是否被传播。** 当前生产代码里两者各恰好出现 2 次（签名 + 调用），**正好卡在阈值上** —— 任何一次「挪走真调用 + 补一次诱饵」都能让计数不变。

**R5 的严重性**：AC-101 的 fail-closed 层被改成 fail-open（读回不是 `on` 也照样勘查客户库），**而 160 个测试一个都没红**。这正是首轮 S1 那个病的同型复发 —— 判据有数，但没有判别力。

**已验证的低成本修法（叠加，不替换）** —— 在现有 4 条判据之外**追加**两条语义断言：

1. 正则 `ensure_read_only\(&read_only\)\?;` 必须出现在剥注释后的生产代码区 → 杀 R5（`.ok()` 不匹配 `?;`）
2. 用 `test_read_only_guard.rs:178-182` **已有**的 `.split()` 手法切出 `read_only_pool` 函数体，要求体内有 `force_read_only(url)?;` → 杀 R4

我的 python 探针实测（`~/req003-review2/work/probe/fixcheck2.py`）：

| 变异 | 现状判据 | **叠加后** |
| --- | --- | --- |
| base（应判绿） | ✓ 绿 | ✓ 绿 |
| R1 删 SET | ✓ 红 | ✓ 红 |
| R2 删 ensure 调用点 | ✓ 红 | ✓ 红 |
| R3 删 force 调用点 | ✓ 红 | ✓ 红 |
| R4 删真调用+诱饵 | 🔴 **绿=漏** | ✓ 红 |
| R5 删校验+`.ok()`诱饵 | 🔴 **绿=漏** | ✓ 红 |
| R6 真调用挪到别的函数 | — | ✓ 红 |

**诚实标注**：只加语义断言会**漏 R1**（after_connect/SET）—— 我第一版就犯了这个错，是叠加而非替换才全绿。

**为什么定为「严重」而不是「阻断」**：① 当前生产代码是**对的**（守卫确实以 `?` 强制），这是**防回归能力的缺口，不是现存缺陷**；② 首轮 S1 的验收标准字面是「那三个变异必须变红」，已达成；③ R5 那种写法（删了再补个 `.ok()`）不像自然演进。但它与首轮 S1 是同一个病，且修法约 6 行 —— **建议同批修**。

### 3.3 `verify_selftest.sh` 9 条是否真在测判据

**实跑 11/11 通过，exit 0。** 逐条定性：

| 条目 | 是否真在执行判据 |
| --- | --- |
| AC-002 五条（注入 `std::fs` / 干净代码 / 行首注释 / 行尾注释 / 字符串字面量） | ✅ 真行为测试（注入 + 判据） |
| AC-101「勘查器源码无写操作字面量」 | ❌ **真实文件的静态检查，无注入** —— 过滤器若被改成永不匹配也会 ✓，**判不出鉴别力**（首轮 B2 同型） |
| AC-101 只读判据对真实文件通过（用例 0） | ✅ 不误报检查 |
| AC-101 用例 1/2/3（三个注入） | ✅ 真注入。**用例 1 还额外校验注入确实生效**（要求调用点已消失**且**那句话还在注释里）—— 这条设计很严谨，是本轮亮点 |
| AC-105 实跑 `cargo test --test test_view` | ✅ 真跑行为用例本体 |

**首轮点名的两个绕过形态都被覆盖**：守卫调用点被删（用例 1）、那句话留在注释里（用例 1 的注释剥离）。**存在性检查只剩 1 条**（写操作字面量），比首轮的 2 条好，但没归零。

**AC-105 防空跑自检：真的能挡住，两种形态都实测过**：

| 注入的错过滤器 | cargo 行为 | 自测结果 |
| --- | --- | --- |
| `--test test_view_ZZZ_NOT_EXIST` | rc=101 | 🔴 报 ✗ → **exit 1** |
| `--test test_view ZZZ_NO_SUCH_TEST` | **rc=0** + `0 passed; 21 filtered out` | 🔴 报 ✗ → **exit 1** |

第二个正是任务点名的「过滤器写错 → 0 passed → rc=0」形态 —— **挡住了**（靠 `[1-9][0-9]* passed` 那条正则 + 两个用例名 grep）。这条自测是真的。

**但同一弱点**：`verify_selftest.sh:92-99` 的 AC-101 只读判据与 Rust 侧是**同一套「数出现次数」逻辑**，所以它**同样漏 R4/R5** —— 判定器自测测不出判定器自己的盲区。

---

## 4. 回归检查

### 4.1 12 条新测试的判别力

`git diff 7d12548..75bd985 --name-only` = 4 个文件（`view.rs`/`introspect.rs`/两个测试文件，+466/-16）。新增测试恰 **12 条**，148+12=160 与声称一致：

| 新测试 | 我打的变异 | 结果 |
| --- | --- | --- |
| `view.rs::test_cardinality_cn_translates_every_known_notation` | M1、M5 | 🔴 变红 ✅ |
| `view.rs::test_guard_catches_any_cardinality_notation` | M2、M3 | 🔴 变红 ✅ |
| `view.rs::test_guard_does_not_mistake_timestamps_for_notation` | 未单独打变异 | ⚠️ 探针显示其 10 个样例全部正确，但**4 个已知误杀样例一个都没进去** |
| `introspect.rs::ensure_read_only_rejects_anything_but_on` | 未打变异 | ⚠️ 逻辑上必然红（未实证） |
| `introspect.rs::read_only_pool_really_reads_back_on` | R1 | 🔴 变红 ✅（真行为测试） |
| `test_read_only_guard.rs::stripper_strips_comments_before_matching` | 未打变异 | ⚠️ 未验 |
| `test_read_only_guard.rs::after_connect_..._is_in_executable_code` | R1 | 🔴 变红 ✅ |
| `test_read_only_guard.rs::force_read_only_is_actually_called...` | R3 | 🔴 变红 ✅ |
| `test_read_only_guard.rs::fail_closed_check_is_actually_called...` | R2 | 🔴 变红 ✅ |
| `test_read_only_guard.rs::read_only_pool_itself_is_not_wired...` | 未打变异 | ⚠️ 未验 |
| `test_view.rs::test_view_translates_every_known_cardinality_notation` | M1、M5 | 🔴 变红 ✅ |
| `test_view.rs::test_view_no_cardinality_notation_passes_silently` | M2、M3、M6 | 🔴 变红 ✅ |

**无互相依赖**：M1 组与 M2/M3 组红在不同测试（已证互不遮蔽）。**未见假断言** —— 唯一存疑的是上表 4 条未单独打变异的。

### 4.2 性能

`cardinality_notation_hit` 是 O(n) 逐字符扫描，但**提前返回**且分配一个 `Vec<char>`：

| 输入 | 用时 | 吞吐 |
| --- | --- | --- |
| 137,969 字节（真实客户视图） | **5.47 ms** | 25.2 MB/s |
| 6.9 MB（真实视图 ×50） | 203 ms | 33.9 MB/s |

**近线性**（50× 输入 ≈ 37× 时间），**不是瓶颈**。真实场景一次调用 5.5 ms，可忽略。
唯一的资源代价是一次 137KB 的 `Vec<char>` 分配（~550KB 临时内存），无泄漏。

### 4.3 `ensure_read_only()` / `read_only_pool()` 抽取有没有改变行为

**没有。** 抽取是纯粹的「提取不改语义」：
- `introspect()` 走 `read_only_pool(url)`（`introspect.rs:177`）→ 内部 `force_read_only(url)?` → `after_connect(SET …)` —— **与测试走的是同一条路径**（`read_only_pool_really_reads_back_on` 直接调 `read_only_pool`）。
- 抽取前 `introspect` 是唯一调用方，抽取后仍是唯一生产调用方（+ 测试调用）。
- `max_connections(2)` / `acquire_timeout(30s)` 是抽取时**新写**的，但那是连接池策略，不改变只读语义。
- 唯一的行为差异是**连接池更保守**（2 而非 5）—— 对客户源库是改善。

### 4.4 L2 · `semantic_cn` 的 `_ => s.to_string()`

**确认还在**：`view.rs:264`。**影响面比 status.md 描述的更值得注意**：

- 探针实测：`scan_modeling_terms(r#"{"meaning":"belongs_to"}"#)` → **返回空**。即 `belongs_to` **不被任何守门词覆盖**。
- 实测：把真实产物 51 条 `meaning` 全改回 `belongs_to`，CHECKS_ONLY **exit 0**。
- 当前真实产物 `meaning` 全是「归属于」，**零泄漏**；但这是**运气**，不是防线 —— 形状与 B1 完全同源，而本轮**只给基数加了形态守门，没给语义加**。
- 修法与 B1 同构（加一个「已知语义漏翻」的形态/词元判据），约 5 行。**建议列为紧跟项**。

### 4.5 `--accept-all` 是否被本轮波及

**结构上不可能**：`git diff --name-only` 只含 4 个文件，`cli.rs` 与 `src/domain/review.rs` **未被触及**。`--accept-all` 的实现在 `cli.rs`，本轮零改动 → 行为不可能改变。**未实跑该路径**（需源库）。

### 4.6 L3 · 形态守门是 fail-closed 取舍

**评估：取舍正确，不需改。** 「宁可整份视图拒发，也不放行一个记号」—— 在「客户视图是给客户看的最终产物」这个前提下，拒发的代价（重跑一次）远小于泄露内部记号的代价（信任受损、且不可撤回）。这与 S1 的只读红线是同一个优先级排序：**安全方向上宁可失败**。

唯一要补的是**可诊断性**：`cli.rs:521` 已经打印 `⚠ 客户视图泄露了建模术语：{leaked:?}`，形态档的 label 是 `未翻译的基数记号 N:1`（`view.rs:573`），可读性足够，运维能据此定位。**保留。**

### 4.7 L4 · 判据模式第 2 步第 1 项是自比的

**确认，且比 L4 描述的更值得注意。** `e2e-req003.sh:139`：

```bash
checks_step2 "out/e2e-proposal.json" "out/e2e-stats.json" \
  "$(python3 -c "…print(json.load(open('out/e2e-proposal.json'))['project'])")"
```

期望值 `proj` **取自产物自身** → `pr['project']==proj` 恒 ✓。实测：把 `project` 改成 `WRONG_PROJECT_XYZ`，该项仍显示 `✓ 提案 project 字段 == 实际项目（WRONG_PROJECT_XYZ）`。

脚本第 134-137 行**诚实披露了这一点**（好）。但在完整 E2E 下（第 191 行）`$PROJ` 是真建出来的，该项有鉴别力。**鉴别力归零仅限判据模式**，可接受 —— 只是这一项在判据模式下**不该被计入「9/9 通过」的信心**。

### 4.8 L5 · 测试库偶发失败 —— ✅ 已定性（从推断升级为结论）

**抓到了 panic message。** 30 轮定向压测（`test_proposal`/`test_snapshot`/`test_db`/`test_project`/lib）：

| 指标 | 值 |
| --- | --- |
| 有记录的轮次 | 13（10 clean / 3 HIT） |
| 抓到的 panic 总数 | **4** |
| 其中 `ConnectionReset` | **4 / 4（100%）** |
| 语义断言失败 | **0** |

全部 panic 原文：

```
Connect(Io(Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" }))
```

落点：`tests/common/mod.rs:24`「连不上测试库」×2、`tests/test_snapshot.rs:260` ×1、`tests/test_snapshot.rs:324` ×1 —— **全部在建连/查询阶段**。

**结论**：根因 = **30432 代理在并发连接压力下重置连接**，属**环境噪声，非产品缺陷**。命中率与并发强相关（我并发跑变异时明显升高，独跑时 0/3 批次）。status.md 的「倾向 30432 代理抖动，但**是推断不是结论**」**现在可以升级为结论**。

**顺带一条工程建议**：这些测试是 `panic` 而非 `skip`，所以 CI 上偶发红会误导。建议给 `tests/common/mod.rs` 的建连加一次带退避的重试（仅针对 `ConnectionReset`），或在这些测试上标注「环境敏感」。

### 4.9 磁盘事故（自查披露）

BA 指出 `/home/myg133/req003-review2` 一度到 **43G**（10 个 target 目录）。**已清理至 8.0G**：8 个已完结变异的 target + r4/r5 立即回收，`mut-*` 源码（各 1.5M）与 `logs/` 全保留。

**但必须如实报告两点**：
1. **`mavis-trash` 在本机不回收空间** —— 它把目录移到 `~/.local/share/Trash/files`（**同一文件系统**），所以 `/home` used 从 198G → 240G，free 从 172G → 130G，**净效果是搬运而不是释放**。该 Trash 里**清理前已积压 69G**（大量 REQ-002 遗留：`feature-REQ-002.*`、`b2check` 等）。按约束我**没有**改用永久删除。
2. **我的流程缺陷**：每变异独立 target 是 briefing 的硬要求（防 hardlink 穿透），但**没设数量上限**。正确做法应是「跑完即回收、只保留最近 1-2 个」，我做到了后半（已回收 10 个中的 10 个），但**前半段铺得太开**。

**建议父 agent 决策**：是否清空 `~/.local/share/Trash/files`（69G+，含 REQ-002 历史遗留）。这超出我的权限，且属不可逆动作。

---

## 5. 结论

### VERDICT：**有条件通过**

B1、B2 **真修好了**，且不是「看起来修好了」：
- B1：6 个变异全部被抓住，其中 M6 证明不变量测试能发现守门器自己的盲区；真实产物 82.3% 覆盖率、零记号透出。
- B2：四次退出码 0/1/1/0，六种不可解析输入全 1 —— 首轮那个「8 项全 ✗ 而 rc=0」的形态确已消失。

S1 的三个**指定**变异真被抓住，但存在一个我新发现的绕过（R5 = 真 fail-open，160/160 全绿）。

### 阻断项

**无。** 本轮没有发现会阻止合并的缺陷。

### 必须同批修的

| 严重性 | 项 | 修法（已验证） |
| --- | --- | --- |
| **严重** | **只读接线判据数出现次数，可被诱饵调用绕过**（`test_read_only_guard.rs:147,159`；`verify_selftest.sh:92-99` 同源）。R4/R5 实测 160/160 全绿，R5 是真 fail-open | **叠加**两条语义断言（不替换现有）：① 正则 `ensure_read_only\(&read_only\)\?;` 必须在剥注释后的生产代码区；② 用 `.split()` 切出 `read_only_pool` 体内必须有 `force_read_only(url)?;`。约 6 行。python 探针实测：base 仍绿，R1-R6 全红 |

### 建议紧跟（不阻断）

| 严重性 | 项 | 依据 |
| --- | --- | --- |
| 中 | **误杀面收紧**（方案 B：形态档只认 `how_many` 字段的完整值，约 3 行）+ 把 4 个已知误杀样例补进反向测试 | §1.4；当前零误杀，但 `note`/`sample` 是自由文本 |
| 中 | **L2 `semantic_cn` 的 `_ =>` 盲区** —— `belongs_to` 不被任何守门词覆盖，实测 51 条改回仍 exit 0 | §4.4 |
| 中 | **status.md L4 补记更严重的盲区** —— CHECKS_ONLY 对 B1 **零敏感**（51 条改回 `N:1` 仍 exit 0），不只是「第 1 项自比」 | §2.2 |
| 低 | **AC-104 判据口径** —— demand 要求判「覆盖率 <60% 判 ✗」，实现判的是「平均属性数≥3」；实测覆盖率 82.3% 达标，`stats.json` 字段齐全，补判据约 3 行 | §2.4 |
| 低 | **AC-102/103/104/106/107 仍无 Rust 判据测试**（首轮指出，本轮只补了 101/105） | §2.4 |
| 低 | **「勘查器源码无写操作字面量」仍是唯一残留的存在性检查**，无注入证明其过滤器有鉴别力 | §3.3 |
| 低 | **测试库 ECONNRESET 偶发红** —— 根因已定性为 30432 代理；建议 `tests/common/mod.rs` 加针对 `ConnectionReset` 的退避重试，避免 CI 误导 | §4.8 |

### 我没能验证的部分

| 项 | 原因 |
| --- | --- |
| **完整 E2E 实跑**（源库 → 勘查 → 抽取 → 审核 → 冻结 → 视图 → diff） | 无源库连接串（客户提供、不入库）。CHECKS_ONLY 模式已覆盖第 2/6 步判据 |
| **`--accept-all` 行为** | 结构上确认未被波及（`cli.rs`/`review.rs` 未在 diff 内），但未实跑 |
| 自述 5 的「注入 11 处缺陷 exit 1」逐条复现 | 时间预算；我独立复现了四连与 6 种 fail-closed |
| 4 条新测试未单独打变异（`stripper_...`、`read_only_pool_itself_...`、`ensure_read_only_rejects...`、`..._mistake_timestamps...`） | 时间预算；前两条逻辑上覆盖充分，后两条见上表 |
| 误杀收紧方案在真实代码上的效果 | 我不改被审对象；只验证了形态档自身鉴别力（39 项探针） |
| AC-102 勘查元数据完整性（91 表 / 每表列数 > 0） | 需源库 |
| **AC-013** | 按要求不判（需 3 名非开发者外部评审人）—— **需人工** |
| 8 个无中文属性名、关系基数语义正确性（`N:1` 是否标反） | 承自首轮，本轮未做 |
