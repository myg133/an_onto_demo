# QA 验收报告 · REQ-046

- 验收对象：`verify/REQ-046` @ `e0f6553`（基线 `c98a631` = develop）
- 验收人：QA（`mvs_70579a18c2ad4a72a8d0f38a5e5607cd`）
- 工具链：cargo 1.98.1 / rustc 1.98.1，8 核，debug profile（**未用 `--release`、未设 `CARGO_TARGET_DIR`、无符号链接**）
- 结论：**PASS 带非阻断**

---

## 0. 结论摘要

| 维度 | 结果 |
|---|---|
| 2×2 判别力矩阵 | ✅ **4 格全部独立复现，条数与 Dev 报告逐项吻合**；另补出 Dev 矩阵里缺的第 5、6 格 |
| 根因深度（不止 `read_source`） | ✅ 成立，但**处数报错了**：真实代码处是 **7** 不是 10 |
| 修复后零裸调用 | ✅ 只剩 `source_root.rs:61` 一处设计内兜底常量 |
| 两类报错分得开 | ✅ 两类抬头与下一步指引均实测不同；但**该分类零判据覆盖**（存活变异 M2） |
| 「读的是活树」强化证据 | ✅ 独立复现（`test_connect_retry-d11c21d529514294`，11/1，直跑不经 cargo） |
| 口径事故处理 | ✅ 处理得当；1072 的成因已独立定位 |
| 全量与红线 | ⚠️ 本轮 **535 passed / 1 failed / 1 ignored**，唯一失败为**环境类**（DB 锁超时），未复现 Dev 的 536/0/1 |
| 红线 | ✅ 全零改动，无测试删除 / `#[ignore]` |
| 产物 sha | ✅ **两个 sha 独立重跑复现，逐字节吻合** |

**Dev 的诚实度评估：偏高。** 2×2 矩阵是真的（我逐格重跑，条数全中）；否决两个候选的理由站得住；口径事故主动登记；最大未闭合点主动登记且**低估了严重性**（见 F-1，实际比他说的更糟）。

---

## 1. 2×2 矩阵是真的吗？（最要紧）

**是。4 格全部独立复现。** 全部为**直跑二进制、不经 cargo**。

### 1.1 二进制名（独立产出）

| 编译来源 | 二进制名 |
|---|---|
| 修复前（基线 `c98a631`，一次性 worktree 编） | `test_acp_extraction-cf646f8262756d5a` / `test_connect_retry-d11c21d529514294` |
| 修复后（`e0f6553`，一次性 worktree 编） | `test_acp_extraction-cf646f8262756d5a` / `test_connect_retry-d11c21d529514294` |
| 修复后（活树编） | `test_acp_extraction-cf646f8262756d5a` / `test_connect_retry-d11c21d529514294` |

⚠️ **三处编译（修复前/修复后/活树）产出的文件名完全相同** ⇒ 独立坐实 Dev 的论断：**文件名根本看不出二进制属于哪棵树**。这就是事故机制的前提条件。

**实测内嵌编译期路径**（`strings` + 精确 pattern `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/qa-throw-REQ046`，避开判据文件里记录 REQ-044 封口史的注释）：

```
/home/myg133/Projects/Anchnet/Ontology/.review-workspace/qa-throw-REQ046
```

### 1.2 矩阵实测（计数口径：libtest 单二进制 `test result:` 行）

| # | 二进制 | cwd | `test_acp_extraction` | `test_connect_retry` | Dev 声称 | 判定 |
|---|---|---|---|---|---|---|
| 1 | 修复前（已回收 worktree 编） | 非 crate 根 | **27 passed / 1 failed**（28 条） | **8 / 4**（12 条） | 27/1、8/4 | ✅ 一致 |
| 2 | 修复前 | **活树（完好 crate 树）** | **27 / 1** | **8 / 4** | 27/1、8/4 | ✅ 一致 |
| 3 | 修复后（已回收 worktree 编） | 非 crate 根 | **27 / 1**（报**类别 B**） | **8 / 4**（报类别 B） | 27/1 类别 B | ✅ 一致 |
| 4 | 修复后（活树编） | 活树 | ✅ **28 / 0** | ✅ **12 / 0** | 28/0、12/0 | ✅ 一致 |

**格 2 复现细节（Dev 说「最刺眼」的那格）**——旧二进制站在一棵完好的 crate 树正中间，**照样 27/1、8/4，与格 1 逐条相同**。panic 原文：

```
thread 'capabilities_is_a_pure_function_that_reads_no_field' panicked at tests/test_acp_extraction.rs:378:59:
读不到 /home/.../qa-throw-REQ046/src/agent/acp/mod.rs：No such file or directory (os error 2)
```

⇒ 与 briefing 里的事故原文**句式、行号、误导性措辞一字不差**。旧锚点**从不看 cwd**，坐实「结构性脆弱」而非一次运气不好。**Dev 的判断成立。**

### 1.3 我补的 2 格（Dev 矩阵里没有，但更决定性）

| # | 二进制 | cwd | `test_acp_extraction` | `test_connect_retry` | 意义 |
|---|---|---|---|---|---|
| **5** | **修复后（已回收 worktree 编）** | **活树** | ✅ **28 / 0** | ✅ **12 / 0** | ⭐ **这才是「事故被修好」的正面证明**：事故的真实场景正是「回收 worktree 编的二进制被复用」；Dev 的 4 格里格 3 之所以失败是因为他选了「非 crate 根」作 cwd，**没有直接演示事故场景被修复**。修复后二进制 + 活树 ⇒ 全绿。 |
| **6** | 修复后（活树编） | 非 crate 根 | **28 / 0** | **12 / 0** | 编译期根仍活着 ⇒ 走兜底分支，判据跟着编译期树走，仍绿。 |

**格 6 顺带证明兜底分支有效**；`crate_root()` 通道也验过（`test_agent_orchestration-061c12c14e72bc95`，非 crate 根 cwd 下 **36 / 0**）。

---

## 2. 根因是 10 处还是更多？

### 2.1 基线 `c98a631` 的独立核对（严格区分代码 / 注释）

口径：`git grep -n 'CARGO_MANIFEST_DIR' c98a631 -- tests/`，逐行按「trim 后是否以 `//` 开头」定性。

```
  代码 >> tests/test_acp_extraction.rs:376
  代码 >> tests/test_agent_orchestration.rs:1150
  注释 >> tests/test_connect_retry.rs:46
  注释 >> tests/test_connect_retry.rs:55
  注释 >> tests/test_connect_retry.rs:270
  注释 >> tests/test_connect_retry.rs:273
  代码 >> tests/test_connect_retry.rs:306
  代码 >> tests/test_extraction_agent.rs:1107
  代码 >> tests/test_snapshot_write_paths.rs:377
  代码 >> tests/test_snapshot_write_paths.rs:408
  代码 >> tests/test_snapshot_write_paths.rs:886
  小计: 注释=4 代码=7 合计=11
```

### 2.2 🔴 发现 F-3：Dev 的处数自相矛盾，且「10 处」是错的

同一数量在 Dev 的产出里出现**三个不同数字**：

| 出处 | 数字 | 与实测对照 |
|---|---|---|
| briefing / 登记表转述的自述「5 个测试文件共 **10 处**裸用」 | 10 | ❌ 错 |
| 他自己那张表的**逐行相加**（1+1+1+3+1） | 7 | ✅ 对 |
| `tests/test_connect_retry.rs` 代码注释里写的「同一个错锚点在仓内共有 **6 处**」 | 6 | ❌ 错（且与上表矛盾） |
| 他说 `test_connect_retry.rs`「另 **3** 处在注释里」 | 3 | ❌ 错，实际 **4** 处（`:46/:55/:270/:273`） |
| **QA 实测** | **代码 7 / 注释 4 / 合计 11** | — |

「10」最像是 **7 代码 + 3 注释** 的加法（而注释实为 4）。**非阻断**（不影响修复本身，修的范围是对的、也修全了），但：
- `BA/dispatch/req-registry.md` 里 REQ-046 与 REQ-047 **两行**都写了「10 处」，需按实测改成「代码 7 处 / 注释 4 处」；
- 这是本会话第 7 次因数字没标口径引发返工，**Dev 的登记表叙述需要一并订正**。

### 2.3 修复后是否真的零裸调用？

**是。** HEAD 逐行定性：

```
  注释 >> tests/common/source_root.rs:6, :7
  代码 >> tests/common/source_root.rs:61     ← const BUILD_MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
  字符串 >> tests/common/source_root.rs:205   ← format! 里的显示文案，非宏调用
  注释 >> tests/test_connect_retry.rs:46,55,57,276,282,287 / test_acp_extraction.rs:28
```

⇒ **真正的 `env!()` 宏调用只剩 1 处**（`source_root.rs:61`，设计内兜底常量）。**Dev 的说法成立。**

### 2.4 有没有 Dev 漏掉的同类锚点？

| 锚点形态 | 全仓实测 | 判定 |
|---|---|---|
| `env!("CARGO_MANIFEST_DIR")` | HEAD 仅 `source_root.rs:61` | ✅ 已收敛 |
| `option_env!` | **0 处**（`tests/` + `src/`） | ✅ 无同类 |
| `file!()` | **0 处** | ✅ 无同类 |
| `std::env::current_dir()` | 仅 `source_root.rs:109`、`:213`（新模块内部） | ✅ 无同类 |
| `include_str!` | 仍在 9 个测试文件里（如 `test_no_write_path.rs`、`test_agent_orchestration.rs`） | ⚠️ **不同性质，不是 REQ-046 的洞**：编译期内嵌、二进制自包含 ⇒ **不受 worktree 回收影响**。但它是**另一种陈旧**（内容冻在编译那一刻），REQ-045 已在 `test_connect_retry.rs` 文件头论证并显式划界。**不算漏修，算已知取舍。** |
| `env!("CARGO_BIN_EXE_ontok-propose")` 等 | 多个测试文件在用 | ⚠️ **同类（编译期绝对路径）但不同性质**：指向构建目录，经 cargo 跑必被重新生成；直跑复用二进制时它同样会失效。**与 REQ-046 的事故机制同源，但修法不同（cargo 集成测试的固有约束）。建议登记为已知项，不在本轮 scope。** |

**结论：Dev 没漏掉同类锚点。** 「不止 `read_source`」这个根因判断成立且完整。

---

## 3. 两类报错真的分得开吗？

### 3.1 两类抬头实测（原文照录）

**类别 A（文件真不存在）** —— fixture：cwd = 一个 `name = "ontok"` 但**缺 `src/agent/acp/mod.rs`** 的目录；二进制的编译期根已回收 ⇒ 单候选。触发点 `source_root.rs:179`：

```
┌─ 【失败类型】**文件真的不存在**（crate 根是**对**的，文件不在）
│  · 已定位到 crate 根：…/fakeA
│    这个根怎么来的：**运行时工作目录上溯**匹配到 `name = "ontok"` 的 manifest
│  · 相对 crate 根：src/agent/acp/mod.rs
│  · 绝对路径：…/fakeA/src/agent/acp/mod.rs
│  · 底层 IO 错误：No such file or directory (os error 2)
│  · 附带信息：编译期目录 `…/qa-throw-REQ046` **已不存在** …
└─ 【怎么修】确认路径拼写；确属改名 / 移动，则同步更新调用它的那条判据。
⚠️ **别把它读成 REQ-046 事故那种「目录不对」**…
```

**类别 B（目录不对）** —— cwd = 非 crate 根。触发点 `source_root.rs:175`：

```
┌─ 【失败类型】**二进制与当前目录不匹配** —— **这不是「源码文件被删了」**
│  · 本二进制**编译时**的 crate 根：…/qa-throw-REQ046
│    现状：**该目录已不存在**（worktree 被回收 / 仓库被搬走 / 构建产物被跨目录复用）
│  · 你**运行它时**的工作目录：…/noncrate
│    现状：该目录及其上溯 8 层内，没有 `name = "ontok"` 的 `Cargo.toml`
│  · 本二进制**自身**位置：…/postfix-throw-test_acp_extraction-cf646f8262756d5a
└─ 【怎么修】二选一：1. cd 到本仓库的 crate 根目录… 2. 在你想检验的那棵树上执行 cargo test 重编 …
⚠️ 判别提示：**类别 B 的特征是「crate 根本身就没找到」**…
```

**抬头不同 ✅ / 下一步指引不同 ✅（A 说「改路径拼写或改调用点」，B 说「cd 过去或重编」）。**

**两个 B 落点都实跑过**：`read_source` 的 `source_root.rs:175` 与 `crate_root()` 的 `source_root.rs:194`（用回收 worktree 编的 `test_agent_orchestration-061c12c14e72bc95`，抬头文案一致、路径项写「此处要的是 crate 根本身，不涉及具体文件」）。

### 3.2 边界情形

| 情形 | 实测归属 | 判定 |
|---|---|---|
| **EISDIR（目标路径是目录）** | **类别 A**，errno 行显示 `Is a directory (os error 21)` | ✅ 归类正确。⚠️ 抬头写「文件真的不存在」措辞不精确（文件存在、是目录），但 errno 行已消歧。**非阻断。** |
| **包名不符的诱饵 crate**（`name = "some-other-crate"`，且 `src/agent/acp/mod.rs` **存在**） | **类别 B**，明确报「上溯 8 层内没有 `name = "ontok"` 的 Cargo.toml」 | ✅ 包名守卫有效，**没有**误读诱饵树 |
| **编译期根缺文件（两候选都失败）** | **未能实跑** | 见下 |

### 3.3 🔴 发现 F-2：Dev 登记项 #2 的描述与代码不符（低）

Dev 说：「`last_err` 取自**最后一个**候选、报错根取**第一个**候选」。**读代码不成立**：

- `source_root.rs:169`：`Err(e) => last_err = Some((root.clone(), e));` —— **root 和 e 打包在同一个元组里**
- `source_root.rs:178-181`：`let (root, e) = last_err.expect(...)` —— 报错的 root **就是**产生该 err 的那个候选

⇒ **二者永远来自同一候选，结构上不可能不一致。** `roots.first()`（`:173`）只用来判 `None`/`Some`，不参与选根。**Dev 对自己代码的这条描述是误读，登记项 #2 可以撤销。**

（真正存在、但我**未能实跑**的边界：两个候选**都**失败时，报出来的 root 是**最后一个**候选（编译期树）而不是主锚（cwd 树）。要构造它必须让编译期根「活着但缺文件」，而我的二进制编译期根要么是活树（有文件）要么已回收（不入候选）⇒ 需破坏活树，QA 不做。**静态判定：分类仍正确（A/B 不变），只是「已定位到 crate 根」那一行指向的是编译期树，可能把人引去看错的那棵树。展示层问题，非阻断。**）

### 3.4 判别力：这个分类有没有存活变异？

**有。** 变异 **M2**（把类别 B 的抬头改成类别 A 的文案，即「两类混成一条」）：

| 项 | 内容 |
|---|---|
| 变异 | `source_root.rs:229` 的 `【失败类型】` 行替换成类别 A 文案 |
| 变异体 | `tests/common/source_root.rs`（在**独立副本**上施加，**未改交付物**） |
| 执行方式 | `cargo test --no-fail-fast`，全量 |
| 结果 | 见 §7 变异表 |
| 论证 | 若存活 ⇒ **AC-003 要求的「区分两类失败」没有任何判据在守**。结构性佐证：`grep -rn '失败类型\|二进制与当前目录不匹配\|文件真的不存在' tests/ --include=*.rs \| grep -v source_root.rs` 只命中 `test_connect_retry.rs:321-322` 的**文档注释表**，**零可执行断言**。 |

---

## 4. 「读的是活树」这条强化证据成立吗？

**成立，已独立复现（直跑，不经 cargo）。**

```
注入前：target/debug/deps/test_connect_retry-d11c21d529514294
        → test result: ok. 12 passed; 0 failed; 0 ignored（12 条）
注入：  往活树 tests/test_db.rs 追加
          fn qa046_mutation_direct_connect_site() { let _s = "connect("; }
          （选可编译写法：判据是**文本级**检查，`normalize` 只剥 `//` 开头的行）
直跑：  同一颗二进制，**不经过 cargo**（cargo 可能重编译掩盖问题）
        → test test_every_test_side_connect_site_goes_through_retry ... FAILED
        → test result: FAILED. 11 passed; 1 failed; 0 ignored（12 条）
还原：  git checkout -- tests/test_db.rs → 复跑 12 / 0 ✅
```

⇒ 判据读的是**当前这棵树**，不是编译它时那棵树。**Dev 的「11/1」逐条吻合。**

⚠️ 一个必须记的口径：本判据是**文本级**的，注入字符串字面量即被捕获 —— 它证明的是「判据读的是活树内容」，**不**证明它做了语法分析。

---

## 5. Dev 那个「口径事故」处理得对吗？

### 5.1 独立复核全量真实数字

**口径 A（逐行解析，兼容 `ok.` 与 `FAILED.`）**：
```
grep 'test result:' full-run-1.log | awk '{...}' END{printf "行数=%d passed=%d failed=%d ignored=%d"}
→ 行数=36 passed=535 failed=1 ignored=1
```
**口径 D（完全独立：`grep -o` 抽数字 + `paste -sd+ | bc`）**：
```
grep 'test result:' … | grep -oE '[0-9]+ passed' | grep -oE '[0-9]+' | paste -sd+ | bc  → 535
grep 'test result:' … | grep -oE '[0-9]+ failed' | grep -oE '[0-9]+' | paste -sd+ | bc  → 1
```
两口径一致。**真实数字：535 passed / 1 failed / 1 ignored**（失败为环境类，见 §7）⇒ 测试总数 **536 + 1 ignored = 537**，与 Dev 的 536/0/1 **总数相同**。

⚠️ **36 这个数不是「36 个二进制」**：实测 `^     Running` 行 = **35**，`test result:` 行 = **36**，差额是 `Doc-tests ontok`（`:816`）。表述应为「35 个二进制 + 1 个 doc-test = 36 条汇总行」。`REQ-043-dev-EVIDENCE.md:84` 写「36 二进制」不够精确（沿用即可，非本轮问题）。

### 5.2 1072 是怎么来的（已定位）

实测 `paste` 行为：

```
$ paste -sd+ f1 f2 | cat -A
1+2+3+4+5$
1+2+3+4+5$          ← 对每个输入文件各输出一行，不是拼成一整行

$ printf '536\n' > d; cat d d | paste -sd+ | bc
1072                 ← 内容重复一次 ⇒ 恰好 2 倍
```

⇒ **`paste -sd+` 对多个文件逐行输出；只要文件列表里同一份数字出现两次（`tee` 重复落盘、`*.log` 通配同时命中运行日志与其副本等），求和结果就精确翻倍。** 1072 = 2 × 536，与 Dev 的第一次汇总吻合。**Dev 的「这个管道有鬼已弃用」判断正确。**

### 5.3 处理够不够？有没有别的地方被同一管道污染？

**够，但有两点应记：**

1. ✅ **做对了**：自查发现 → 弃用该管道 → 换**两个独立口径**复核 → 主动登记。顺序正确。
2. ⚠️ **他没做的一步**：应把「哪些数字是用这个管道算的」列一遍并逐个换口径。我代为核了：**2×2 矩阵的 27/1、8/4、28/0、12/0 全部未受污染** —— 这些数字直接来自 libtest 单二进制 `test result:` 行（不可能被跨文件求和放大），且与我独立重跑逐项吻合；`536` 也恰好是 `1072` 的一半，自洽。**结论：污染仅限那一次汇总，无扩散。**
3. ⚠️ 我自己也踩了一次同类坑（见 §9 自引问题 #2），**同一类管道错误在本会话反复出现** ⇒ 建议 BA 把「量化数字必须标口径与命令」写进 dispatch/rules.md 的硬规则，而不是只留在 briefing 里。

---

## 6. REQ-047 值得单开吗？

### 6.1 Dev 的判断：**对，而且他低估了严重性**

Dev 说「没有守卫，修复随时可被一行绕过」。**实测比这更糟**：

变异 **M1** 把 `candidate_roots()` 的 cwd 分支删掉（锚点退回编译期常量 = **REQ-045 的行为**），**全量仍 536/0/1，一条测试都没红**。

⇒ 不只是「下一个人可能再写裸 `env!()`」，而是 **REQ-046 的修复本身被整体回退，536 条测试也发现不了**。**AC-001/AC-002 目前是零回归覆盖的。** REQ-047 因此**必须单开，且优先级应高于 Dev 的自评。**

### 6.2 守卫判据该怎么写才不会自己也有洞

Dev 想要的「grep 禁止裸 `env!`」是必要条件，但**单独用会有洞**：

| 绕过手法 | grep 守卫能挡住吗 |
|---|---|
| 直接写 `env!("CARGO_MANIFEST_DIR")` | ✅ 挡住 |
| 换写法 `std::env::var("CARGO_MANIFEST_DIR")` | ❌ 漏 |
| 包一层 helper 再用 | ❌ 漏（除非守卫同时锁 helper 清单） |
| `option_env!` | ❌ 漏 |
| **把已有的正确实现改回编译期锚点（M1）** | ❌ **完全漏** |

⇒ 建议 **三条判据分层**，缺一条就有洞：

**G-1 · 语法层（挡「再写裸 env!」）**
- 口径：递归扫 `tests/**/*.rs` 的**代码行**（必须先剥 `//` 与 `/* */` 注释行——这正是本会话已犯过的口径错）。
- 断言：`env!("CARGO_MANIFEST_DIR")` 只允许出现在 `tests/common/source_root.rs`，且**该行必须带一个锚注释标记**（如 `// ANCHOR-ALLOW`）；其它任何文件出现即红并打印 `file:line`。
- ⚠️ 自身漏洞：写 `env! ( "CARGO_MANIFEST_DIR" )`（带空格）、字符串拼接、`option_env!` 会漏 ⇒ 必须用**词法级**匹配（先做去注释 + 归一空白），不是裸 `grep`。

**G-2 · 行为层（挡 M1 —— 这条最有牙齿）**
- 断言 `tests/common/source_root.rs` 的 `candidate_roots()` 里，**`crate_root_from_cwd()` 的结果排在 `BUILD_MANIFEST_DIR` 之前**；且 `MAX_WALK_UP` 有上界。
- 实现形态：对 `source_root.rs` 做归一化后做**顺序断言**（`find()` 比较下标），不做脆弱的逐行相等。
- ⇒ **能直接杀死 M1。**

**G-3 · 契约层（挡 M2 —— 两条文案必须不同）**
- 把两段诊断的抬头**抽成可被测试调用的纯函数**，新增一条判据：分别喂「无可用根」与「根在、文件不在」两种输入，断言 **两个抬头互不相等**，且分别包含约定的判别短语。
- ⚠️ **实现约束（重要）**：不要在测试里用 `std::env::set_current_dir` 造 cwd —— 它是**进程级全局**，会和其他并行测试互相污染。要么把定位逻辑重构成「吃一个 base 参数的纯函数」，要么单独开一个只有这一条判据的测试二进制。
- ⇒ **能杀死 M2。**

**优先级建议：G-2 > G-3 > G-1。** G-1 防的是「将来」，G-2/G-3 防的是「现在就已经没人守的地基」。

### 6.3 加守卫会让基线 536 → 537，哪些地方要同步改

⚠️ 若按 G-1+G-2+G-3 三条都加，则是 **536 → 539**（+3）；只加一条则是 537。**BA 请先定条数，再按下面清单改。**

需同步的位置（`BA/` 分支，实测 grep "536" 命中）：

| 文件 | 行 | 现值 |
|---|---|---|
| `BA/demands/REQ-046/acceptance.md` | 84 | `预期结果：**536 passed / 0 failed / 1 ignored**` |
| `BA/demands/REQ-045/acceptance.md` | 105 | 同上 |
| `BA/demands/REQ-045/acceptance.md` | 106 | 基线 `0c00428` = **536/0/1，5m50s** |
| `BA/demands/REQ-044/acceptance.md` | 94 | 同上（本需求若不新增判据则仍是 536） |
| `BA/demands/REQ-043/acceptance.md` | 13 | `全量两轮 534/0/1 与 536/0/1` |
| `BA/demands/REQ-043/acceptance.md` | 106 | `预期结果：**536 passed / …**` |
| `BA/dispatch/req-registry.md` | 96 | REQ-043 行：「全量 **536/0/1**」「47 次变异执行全 `cf=False`」 |
| `BA/dispatch/req-registry.md` | 100 | REQ-045 行：「全量 **536/0/1** 两轮同值」 |
| `BA/dispatch/req-registry.md` | 102 | **REQ-047 行**：「基线条数将 **536 → 537**」「需同步更新所有引用 536 的验收标准与登记表基线」 |
| `BA/dispatch/req-registry.md` | 108 | REQ-046 行：「**10 处**裸用」⚠️ 见 F-3，需一并订正为 7 |
| `BA/docs/reports/evidence/2026-10-09-REQ-043-dev-EVIDENCE.md` | 84 | 「36 二进制 / **536 passed / 0 failed / 1 ignored**，6m12.031s」 |
| `BA/docs/reports/evidence/2026-10-09-REQ-043-qa-verify043-REPORT.md` | 242 | 「**536 passed / 0 failed / 1 ignored**，36 个 `test result:` 行」 |
| `BA/docs/reports/evidence/2026-10-09-REQ-044-qa-verify044-REPORT.md` | 222 | BASELINE `a1089df`：536 / 0 / 1 / 351s |
| `BA/docs/reports/evidence/2026-10-09-REQ-045-qa-verify045-REPORT.md` | 254, 303, 304, 306, 325, 436 | AC-006 全量与红线多处 |

**判定原则建议**：**历史报告（`docs/reports/evidence/*`）不回改** —— 那是当时的实测记录，改了就失去证据价值；只改**活文档**（`acceptance.md`、`req-registry.md`、`status.md`、`design-summary.md`）与 REQ-047 自己的验收标准，并在 REQ-047 里写明「基线由 536 变为 N，原因：新增 G-x 守卫判据」。

---
## 7. 变异测试表

**执行纪律**：每条变异都**真的跑过**并给出二进制名与运行条数；变异施加在**独立源码副本**上（`.review-workspace/verify-req046/mutant{,2}`），**交付物 `verify-REQ-046` 一行未改**（结尾 `git status` 已验证）；**编译失败不算变异执行**（M1、M2 均正常编译并跑完全量 36 条汇总行）。

| # | 变异 | 施加位置 | 执行方式 | 结果 | 判定 |
|---|---|---|---|---|---|
| **M1** | 删掉 `candidate_roots()` 的 cwd 分支，**锚点退回编译期常量**（＝还原 REQ-045 行为） | `tests/common/source_root.rs:129-137`（副本） | `cargo test --no-fail-fast` 全量 | **536 passed / 0 failed / 1 ignored**，退出码 0，**7m44.388s** | 🔴 **存活** |
| **M2** | 把**类别 B 的抬头替换成类别 A 的文案**（两类合并成一条） | `tests/common/source_root.rs:229`（副本） | `cargo test --no-fail-fast` 全量 | **536 passed / 0 failed / 1 ignored**，退出码 0，**7m19.097s** | 🔴 **存活** |
| **M3** | 往活树 `tests/test_db.rs` 注入裸 `connect(` | 活树（已还原） | **直跑 `test_connect_retry-d11c21d529514294`，不经 cargo** | **11 passed / 1 failed**（12 条），`test_every_test_side_connect_site_goes_through_retry` 红 | ✅ **杀死** |

### 7.1 存活变异逐条论证

#### M1 · 存活 ⇒ **是「判据有洞」，不是等价变异**

- **不是等价变异**：锚点从「运行时重新定位」退回「编译期常量」，正是 REQ-045→REQ-046 之间的**全部功能差异**。§1 的格 1/2 与格 5 已**实跑证明**这个差异在事故场景下产生 4 条 + 4 条真实失败。变异保留了「能读到文件」的能力，但**丢掉了 REQ-046 唯一的实质贡献**。
- **判据确实有洞**：`cargo test` 起的测试进程 **cwd 恒等于包根**，且包根必然存在 ⇒ `crate_root_from_cwd()` 与 `BUILD_MANIFEST_DIR` **在正常路径上取值相同**（这正是 `source_root.rs:29-31` 自己的论证）。⇒ **536 条判据里没有任何一条能在 cwd ≠ 包根、或编译期根已消失的条件下运行**，因此 AC-001 / AC-002 目前是**零回归覆盖**。
- **影响**：REQ-046 的修复可被整体回退而不被发现。**这直接支撑 REQ-047 单开的必要性，且优先级高于 Dev 的自评**（Dev 只想到「新代码再写裸 env!」，实际是「已有修复本身无保护」）。

#### M2 · 存活 ⇒ **是「判据有洞」**（AC-003 无守卫）

- **不是等价变异**：它把 AC-003 明确要求的两类失败合并回事故里「最有害的那一条」笼统报错。
- **判据确实有洞**：两条文案在全仓**零可执行断言**（`grep` 只命中 `test_connect_retry.rs:321-322` 的文档注释表）。文案只在 panic 分支产出，而 §3 已证明**正常路径永不进入 panic 分支** ⇒ 退化不可见。
- **影响**：下一次有人「简化一下报错文案」，事故里最有害的那部分会**静默复活**，而 536 条全绿。

**两条存活变异合起来的结论：REQ-046 的**行为**正确、**判别力矩阵**是真的，但 REQ-046 **自身的修复与新引入的分类，都还没有任何东西在守**。这与 Dev 主动登记的最大未闭合点同向，**且比他说得更严重**。

---

## 8. 测试与环境（含基线对照）

### 8.1 全量结果

| 轮次 | 树 | 命令 | passed | failed | ignored | 汇总行 | 耗时 |
|---|---|---|---|---|---|---|---|
| **QA 第 1 轮（交付物）** | `e0f6553` 活树 | `dbrun.sh cargo test --no-fail-fast` | **535** | **1** | **1** | 36 | **9m57.083s** |
| QA 补跑 `test_db` 单独 ×2 | 同上 | 直跑 `test_db-1f2c466192d32e2d` | 11/11 | 0 | 0 | — | 15.75s / 23.76s |
| **M1 变异轮** | 副本（锚点回退） | `cargo test --no-fail-fast` | **536** | **0** | **1** | 36 | 7m44.388s |
| **M2 变异轮** | 副本（文案合并） | `cargo test --no-fail-fast` | **536** | **0** | **1** | 36 | 7m19.097s |

⚠️ **计数口径**：`test result:` 行逐行解析（兼容 `ok.` 与 `FAILED.`），两套独立口径交叉验证；36 = **35 个 `Running` 二进制 + 1 个 `Doc-tests ontok`**。

### 8.2 ⚠️ 我未复现 Dev 的 536/0/1 —— 唯一失败定性

**`test_migrate_takes_no_access_exclusive_lock_on_settled_schema`（`tests/test_db.rs:596`）**

```
panicked at tests/test_db.rs:596:9:
稳态库上 migrate() 没能跑完（table=ontology_versions, Err(... code: "55P03",
message: "canceling statement due to lock timeout" ...))
```

**定性：环境类失败，不是代码失败。** 证据：

1. `git diff --name-only c98a631 HEAD` **不含 `tests/test_db.rs`** ⇒ 本需求根本没碰这个文件；
2. 错误是 **`55P03` 锁超时**，同二进制内另一条 `test_migrate_on_empty_schema_survives_concurrent_first_run` 日志显示 `has been running for over 60 seconds` ⇒ **测试内并发 + 共享集群**导致；
3. **单独跑 2 次全过**（11/11，15.75s / 23.76s）；
4. **M1 变异轮全量跑时这条也通过** ⇒ 失败与否与 REQ-046 无关。

⚠️ **复现率随负载浮动，不声称稳定值**：本会话 3 次全量里出现 **1 次**（1/3），单独跑 0/2 失败。

**因此：本轮验收的诚实结论是 `535 passed / 1 failed（环境类）/ 1 ignored`，而不是 Dev 的 536/0/1。测试总数与基线一致（536 + 1 ignored），红线上没有回归。**

### 8.3 基线对照（**用 M1 同会话对照替代，并说明局限**）

⚠️ **未跑基线提交 `c98a631` 的完整全量**（需另建 worktree + 另一次 ~8 分钟跑）。我提供的是**更强但不同**的对照：

- **M1 轮把 REQ-046 的锚点修复整体回退到 REQ-045 行为**，同会话跑出 **536/0/1**，与交付物树**逐项同值** ⇒「本次改动对全量可观测行为零影响」这一条被**实跑**证明。
- **测试集合结构等价**：`#[test]` / `#[tokio::test]` 属性数 **基线 432 = HEAD 432**；函数名集合差集只有 `read_source`（旧实现被迁走，**非测试**）与 `source_root.rs` 新增的 8 个 helper。
- **未验证**：基线提交本身在本会话的独立全量数字。**若 BA 需要该数字，请单独排一轮。**

### 8.4 环境事实（独立实测）

| 项 | 实测 | 口径 |
|---|---|---|
| `psql` | **不存在**（`which psql` 无） | — |
| `psycopg` / `psycopg2` | **均不存在**（`ModuleNotFoundError`） | ⇒ **`SELECT 1` p50 本机测不了，Dev 的这条自述属实** |
| TCP 建连 p50 | **39.22 ms**（min 34.04 / max 59.96） | 口径：**到 `172.16.120.20:30432` 的 TCP 握手，不含认证与查询，n=10** |
| 与 briefing 的 133~161 ms 比较 | **不可比** | 那是 `SELECT 1` 的往返口径；与 Dev 的 37.7 ms 同量级，口径相同 |
| DB URL 库名 | `.env` 里是 **`notol`**，需替换成 `ontol` | **Dev/QA 踩坑记录属实**；我用只读包装脚本在内存里替换，**从不 echo、从不落盘** |
| 共享本体库 | **未执行任何 DELETE / TRUNCATE / 清空**；源库只读 | — |

### 8.5 红线核查

| 红线 | 实测 | 判定 |
|---|---|---|
| `src/` 零改动 | `git diff --stat c98a631 HEAD -- src/` **输出为空** | ✅ |
| `tests/drop_rate.rs` 零改动 | 同上，输出为空 | ✅ |
| `tests/test_fk_naming.rs` 零改动 | 同上，输出为空 | ✅ |
| `test_humanize_still_refuses_unknown_columns` 一条断言都没放宽 | 该测试在 **`src/domain/view.rs:1254`**（`src/` 内的单元测试，非 `tests/`）；`src/` 逐字节未改 | ✅ **结构性成立** |
| 无测试被删除 | `#[test]`/`#[tokio::test]` 属性 **432 = 432**；函数名差集仅 `read_source`（实现迁走，非测试） | ✅ |
| 无 `#[ignore]` | `git grep -c '#\[ignore'` **基线 2 = HEAD 2** | ✅ |
| 判据文件内测试条数不变 | 36 = 36（两轮同值，见 §8.1） | ✅ |
| 产物 sha 不变 | 见 §8.6 | ✅ |

### 8.6 产物 sha（独立重跑，非采信现存文件）

命令（**实属 `ontok-propose`；`--out` 在 `--stats` 之前；不传 `--names`**）：

```
./target/debug/ontok-propose \
  --schema …/code/out/e2e-schema.json \
  --out out/proposal.json --stats out/stats.json --project extracted
```

| 产物 | 实测 sha256 | 期望 | 判定 |
|---|---|---|---|
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | 同 | ✅ |
| `proposal.json` | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | 同 | ✅ |

✅ **逐字节吻合。** 输入 `code/out/e2e-schema.json` 为 **91 表**，与产出 `stats.json` 的 `tables_seen: 91` 自洽。

⚠️ **环境限制（如实登记）**：`ontok-introspect` 直连源库报 `Connection reset by peer` ⇒ **我无法自己生成 schema 输入**，改用仓内既有的 `code/out/e2e-schema.json`。**这是「环境类不可用」，不是产品失败**；产物 sha 的可复现性因此建立在一个**已存在的 schema 快照**上，而非我现场内省得到。口令全程只走环境变量，**未写入本报告**。

---

## 9. 发现但未修的问题（按严重度）

| ID | 严重度 | 问题 | 触发条件 | 影响 | 证据 |
|---|---|---|---|---|---|
| **F-1** | 🔴 中 | **REQ-046 的修复与新分类都零回归覆盖**：M1（整体回退锚点）与 M2（合并两类报错）**双双存活，全量 536/0/1** | 任何人回退 `candidate_roots()` 或改写诊断文案 | AC-001/AC-002/AC-003 全部可无声退化；事故里「最有害的部分」会静默复活 | `mutant-M1.log` / `mutant-M2.log`（两轮退出码 0） |
| **F-2** | 🟡 中 | **静默回落 ⇒ 假绿**：cwd 是另一个 `ontok` 根、文件不在那里时，`candidate_roots()` 会回落到**编译期根**并成功返回，判据**绿灯** | 从 worktree B 运行「在 worktree A 编的」二进制，且目标文件在 B 里被改名/移动 | 判据**读的不是你脚下那棵树**，且对「文件被改名」**不报 fail-closed** —— 正是 `source_root.rs:149-151` 说要防的情形。且 `source_root.rs:33-36` 的论证「核对包名把静默读错那棵树变成一次响的失败」**只对『别的 crate』成立**，对**同一个 crate 的另一个 worktree 不成立** | 实跑：cwd=`fakeA`（`name="ontok"`、缺 `src/agent/acp/mod.rs`）+ 活树二进制 ⇒ `ok`（28 条里那条 **passed**） |
| **F-3** | 🟡 低 | **Dev 的处数自相矛盾且「10 处」是错的**：实测基线 **代码 7 处 / 注释 4 处 / 合计 11** | 引用该数字的登记表与验收文档 | `req-registry.md` REQ-046 与 REQ-047 两行的「10 处」需订正 | §2.2 逐行分账；`test_connect_retry.rs` 代码注释里又写「6 处」 |
| **F-4** | 🟢 低 | **两条文案零判据覆盖**（AC-003 无守卫） | 同 F-1 | 同 F-1（M2 已实跑坐实） | `grep` 只命中文档注释；M2 存活 |
| **F-5** | 🟢 低 | **Dev 登记项 #2 与代码不符**，可撤销 | —— | 误导后续维护者去查一个结构上不存在的不一致 | `source_root.rs:169` 与 `:178-181`（root 与 e 同元组） |
| **F-6** | 🟢 低 | EISDIR 时抬头写「文件真的不存在」措辞不精确 | 目标路径是目录 | 仅措辞；errno 行（`os error 21`）已消歧 | 实跑 category A，errno 21 |
| **F-7** | 🟢 低 | 「两候选都失败」边界**未能实跑** | 需要编译期根「活着但缺文件」 | 静态判定：分类仍正确，但「已定位到 crate 根」会指向**编译期树**而非主锚 | 需破坏活树，QA 不做 |
| **F-8** | ⚪ 信息 | `include_str!` 仍留在 9 个测试文件（不同性质：编译期内嵌、不受 worktree 回收影响） | —— | **不是本轮漏修**；属另一种陈旧，REQ-045 已显式划界 | §2.4 |
| **F-9** | ⚪ 信息 | `env!("CARGO_BIN_EXE_*")` 与 `CARGO_MANIFEST_DIR` **同类**（编译期绝对路径） | 直跑复用二进制 | cargo 集成测试固有约束，经 cargo 必重生成；**建议登记为已知项** | 多个测试文件在用 |

---

## 10. 我自己引入又清掉的问题（如实登记）

| # | 我做的事 | 处置 | 是否影响结论 |
|---|---|---|---|
| 1 | **误用 `/tmp`** 写了两份对照文件（`qa046-base-fns.txt` / `qa046-head-fns.txt`），违反「禁 `/tmp`」 | 立即用 `mavis-trash` 移到回收站并确认消失，改在 `.review-workspace/verify-req046/` 重做 | ❌ 不影响 |
| 2 | **自造了一个与 Dev 同类的口径错**：第一版第二口径用 `sed -E 's/.*ok\. …/\1 \2 \3/'`，只匹配 `ok.`，把 `FAILED.` 行整条丢掉 ⇒ 得 **525/0**，比真值少 10 | 发现后弃用该 sed，改用兼容两种前缀的 awk（口径 C）并加第三套 `grep -o`+`paste` 交叉验证 ⇒ 535/1 | ❌ 不影响（已弃用；**恰好印证 §5.3 的建议：这类管道错误在本会话反复出现，应写进 rules.md**） |
| 3 | **第一次跑全量忘了注入 DB 环境变量** ⇒ 大批 DB 测试秒败 | 补只读包装脚本（内存替换 `notol`→`ontol`，从不 echo/落盘）后重跑 | ❌ 不影响（首轮已作废，未计入 §8.1） |
| 4 | **M1 变异轮唯一的失败是我自己副本位置造成的假象**（`drop_rate` 找 `../.review-workspace/fix-props/schema.json`，副本的 `..` 指向别处） | 补 `ONTOK_SCHEMA_JSON` 重跑该二进制 ⇒ 1 passed；合成 **536/0/1** | ❌ 不影响，但**若不追查就会被误判成「M1 杀死了一条测试」** |
| 5 | **本 session 没有 `edit` / `write` 工具**（工具表只有 read/bash/grep/glob/task_*/web_fetch），`edit` 调用返回 `Tool edit not found` | 改用 `python3` 精确替换 + 带引号 heredoc 写报告；**偏离已登记** | ❌ 不影响 |
| 6 | 两次 `mavis-trash` 批量删除因路径不存在整体失败（`no files were moved`） | 用正确路径重跑，5 个 fixture 全部清掉 | ❌ 不影响 |
| 7 | 建了 2 个一次性 worktree（`qa-throw-REQ046`、`qa-throw2-REQ046`）用于造「已回收 worktree 编的二进制」 | 各自 `git worktree remove --force` 回收，`git worktree list` 已确认消失 | ❌ 不影响（**这是复现事故机制必需的 fixture，非改交付物**） |

---

## 11. 收尾状态

- 交付物 `verify-REQ-046`：**HEAD = `e0f6553`**，`git status` 仅剩 **未跟踪的 `.review-workspace/`**（本报告与日志，按要求落在此处）。
- 一次性 worktree：**全部回收**（`git worktree list` 无 `qa-throw*`）。
- fixture（`mutant/`、`mutant2/`、`fakeA/`、`fakeE/`、`fakeOther/`、`bin-prefix/`）：**已用 `mavis-trash` 清除**。
- **未触碰** `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`（仅**只读**用于 `ONTOK_SCHEMA_JSON`）。
- 未 push、未 merge、未 squash、未 force、未改历史；仓库无 remote。

---

## 12. 最终判定

**VERDICT: PASS 带非阻断**

REQ-046 的**行为正确、证据真实、口径诚实**：2×2 矩阵四格逐条复现，事故机制独立坐实，修复确实把事故场景从「静默误报文件不存在」变成「明确报目录不对」；三类主动登记（口径事故 / 最大未闭合点 / `.env` 库名）**全部属实**，其中 `.env` 与 psql/psycopg 缺失我已独立复核。

**非阻断项**（建议 BA 记入 REQ-047 与登记表订正）：

1. **F-1（最要紧）**：修复本身与新分类**零回归覆盖**，M1/M2 双存活 ⇒ REQ-047 **必须单开**，且守卫应按 §6.2 的 **G-2 > G-3 > G-1** 分层设计；
2. **F-2**：`source_root.rs:33-36` 的论证口径过强（包名只挡「别的 crate」，挡不住「同一 crate 的另一个 worktree」），静默回落会形成假绿；
3. **F-3**：「10 处」应为**代码 7 处 / 注释 4 处**，登记表两处需订正；
4. 验收数字应记为 **`535/1(环境类)/1`，测试总数 536+1 ignored 与基线一致**，而不是直接采信 536/0/1。
