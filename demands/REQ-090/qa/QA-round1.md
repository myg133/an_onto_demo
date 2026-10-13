# QA 独立验证报告 · REQ-090 `ontok-translate-names`

> 验证者：Verifier（独立复核，无开发方上下文）
> worktree：`feature-REQ-090` · 分支 `feature/REQ-090` · HEAD `09777194716d939d060b4257eaac614163fffb47`
> 基线：develop `958f4424d8356e18c4bb081d834a6f0bb31b034a`
> 验证产物：`.review-workspace/req090qa/`（本目录，含 mock、夹具、捕获、变异补丁、全量日志）
> 验证结束时 worktree 状态：`git status --porcelain -uall` = **0 行**，HEAD 未变，二进制 sha256 回滚后与干净基线**逐字节相同**

---

## 0 · 总判定

> ## **PASS 带非阻断**
>
> **一句话理由**：九条 AC 的**功能主张逐条实测成立**（我自己搭 mock 重跑了 AC-001/002/003/004/005/007/008，
> 并用真实消费方 `ontok-propose` 做了正反双向验证）；全量 **798 passed / 0 failed / 1 ignored**，
> 两条红线测试**转绿**；**唯一实质缺陷是退出码契约在一个边界上仍然重载**（缺口=100% 时 2 退化为 1，
> 且此时 stdout 汇总行与缺口清单双通道同时缺失），属 fail-closed 不被破坏的契约缺口，不构成安全问题。

「带非阻断」的具体含义：**不构成 FAIL 的理由**是——所有缺陷都不产生错误产物（坏产物一律不落盘），
不触碰红线文件，不引入回归；**不该判纯 PASS 的理由**是——AC-002 明写「退出码语义能区分全覆盖与有缺口」，
实测该区分在 100% 缺口时失效，且 AC-002 要求的「两处覆盖率出口」在同一条路径上双双消失。

---

## 1 · 前提复核（briefing 要求先证）

三条前提**全部成立**，可以继续：

| 检查 | 命令 | 期望 | 实测 | 判定 |
| --- | --- | --- | --- | --- |
| skip-worktree 位 | `git ls-files -v -- src/extract/table_names.json` | 以 `S` 开头 | `S src/extract/table_names.json` | ✅ |
| 工作区干净 | `git status --short` | 空 | 空（`--porcelain -uall` 计 0 行） | ✅ |
| 真版翻译表 | `python3 -c "…json.load…"` | `1.2.0 91` | `1.2.0 91` | ✅ |

补充（纪律 K：对照必须用 `git show HEAD:<path>`，不能读工作区）：

| 版本 | version | 条数 | 前 5 条 | sha256 |
| --- | --- | --- | --- | --- |
| 工作区（真版，skip-worktree） | `1.2.0` | 91 | `activity→活动`, `activity_keys→活动关键词` … | `0d8f6b0d9d1bb44c5ff9759e8c5280f5ba6a76e176de913c5f9113e73dd11aa3` |
| 入库版（`git show HEAD:…`） | `1.3.0-desensitized` | 91 | `t001→对象 001`, `t002→对象 002` … | `2bc99219647ac4e5eb8199ad1eed71a732a1c213cb9d9d54f4e3b29adbbe5c9c` |

⇒ 脱敏占位版与真版**表名零重叠**，briefing 描述的失效链条成立。

---

## 2 · 事实核对

| 项 | 命令 | 结果 | 判定 |
| --- | --- | --- | --- |
| 改动是否只有一个文件 | `git diff --name-only 958f442..HEAD` | `src/bin/ontok-translate-names.rs` | ✅ 吻合 |
| 文件规模 | `git diff --numstat 958f442..HEAD` | `964  0  src/bin/ontok-translate-names.rs` | ✅（声称 963 行，实测 **964 行新增**；差 1 行，见 §7-F4） |
| 三个只读文件 | `git diff --numstat 958f442..HEAD -- src/extract/names.rs src/extract/propose.rs src/domain/view.rs` | **空** | ✅ 吻合 |
| 两条红线测试文件 | 同上，`tests/drop_rate.rs tests/test_fk_naming.rs` | **空** | ✅ 红线零改动 |

**AC-006 判定：吻合。** 抽取器零改动成立。

---

## 3 · 全量测试（我的数字）

命令与口径：

```
cd /home/myg133/Projects/Anchnet/Ontology/feature-REQ-090
set -a; . /home/myg133/Projects/Anchnet/Ontology/.env; set +a
cargo test --no-fail-fast        # 未用 -q，未用 2>/dev/null，完整日志落盘
```

* 完整日志：`.review-workspace/req090qa/full-test.log`（1232 行）
* 口径：**逐个 `^test result` 行求和**（共 **60** 个 suite 行），按 suite 名区分 lib / bin / 集成 / doc
* 进程退出码：`0`

| 类别 | suite 数 | passed | failed | ignored |
| --- | ---: | ---: | ---: | ---: |
| lib（`unittests src/lib/…`） | 2 | 131 | 0 | 0 |
| bin（`unittests src/bin/…`） | 4 | 50 | 0 | 0 |
| 集成测试（`tests/*.rs`） | 53 | 615 | 0 | **1** |
| doc-tests | 1 | 2 | 0 | 0 |
| **合计** | **60** | **798** | **0** | **1** |

新增 bin 的单测 suite：`unittests src/bin/ontok-translate-names.rs` → **12 passed / 0 failed / 0 ignored**。

### 3.1 失败清单

**干净全量：0 条失败。** 逐条列不出，因为没有。

### 3.2 两条红线测试：**转绿**

| 红线测试 | 干净全量结果 | 日志出处 |
| --- | --- | --- |
| `tests/drop_rate.rs::real_schema_drop_rate_report` | **ok** | `full-test.log:274` |
| `tests/test_fk_naming.rs::test_real_schema_customer_id_has_exactly_one_display_name` | **ok** | `full-test.log:690` |

（`tests/drop_rate.rs` 整体 1 passed / 0 failed；`tests/test_fk_naming.rs` 整体 6 passed / 0 failed。）

### 3.3 与开发方数字的算术交叉核对

| 来源 | passed | failed | ignored | 可运行测试总数 |
| --- | ---: | ---: | ---: | ---: |
| 开发方声称 | 795 | 3 | 1 | **798** |
| **我实测** | **798** | **0** | **1** | **798** |

⇒ 两次独立运行的**可运行测试总数完全一致（798）**，开发方那 3 条失败恰是我这里转绿的这 3 条。
这在数字层面独立佐证：开发方的 3 条失败**不是**「3 条真缺陷」，而是环境位未重放导致的同一批测试。

### 3.4 与 BA 基线声明的出入（**归属 BA 侧，非本 REQ**）

`BA/demands/REQ-090/acceptance.md:262` 记基线为 develop `958f442` = **780 passed / 0 failed / 1 ignored / 59 suites**。
本 REQ 新增 12 条单测 ⇒ 预期 792，但实测 **798**，**差 6 条**。

* 该差额**不可能由本 REQ 引入**：开发方与我在**同一分支**上各自独立跑出的总测试数都是 798。
* 我**无法**在不改 worktree 的约束下重测 develop 基线（切分支会动工作区），故此项**记为未独立复核**。
* 提请 BA 核对基线数字来源（可能测量口径不同，或基线数字陈旧）。

---

## 4 · 逐条 AC 判定表

| AC | 开发方声称 | 我实测 | 判定 |
| --- | --- | --- | --- |
| **AC-001** | 产物必须能被 `TableNames::load` 原样接受；坏产物被拒且不落盘 | 三种注入全部被拒、**均不落盘**；**真实消费方 `ontok-propose` 双向验证**：好产物 `EXIT=0` 且「翻译覆盖 4 / 4 表」，三份手造坏产物 `EXIT=1` 并指名表名 | ✅ **吻合**（一处措辞偏差见 §7-F1） |
| **AC-002** | 覆盖率两处都有；退出码 0/1/2/3 四码互不重载 | stdout `translated 3 / total 5` ✅；`_coverage.untranslated/absent_from_schema` 机器可读 ✅；部分缺口 ⇒ **2**、承认缺口 ⇒ **3**、全覆盖 ⇒ **0** ✅ | ⚠️ **不完全吻合**：缺口=100% 时退化为 **1**，且两处覆盖率出口同时消失（§7-F2） |
| **AC-003** | 新表补 / 旧表逐字节未变 / `--overwrite` 定点 / `--regenerate-all` 重翻全部 / 已消失的表绝不删且进 `absent_from_schema` | 全部成立；**第二遍只送新表**经**线上证据**（我的 mock 请求日志）独立证实：第 2 遍只收到 `["tbl_refund","tbl_shipment"]` | ✅ **吻合** |
| **AC-004** | `agent` 自报、不得硬编码；`version` 不借用旧表 | 硬编码变异后产物 `agent` 立刻变成 `bin:HARDCODED_LITERAL_NOT_SELF_REPORTED` ⇒ 该机制**可被 grep 检出**（AC 自证要求成立）；喂入 `version="9.9.9-FAKE-OLD-TABLE"` 的旧表，新产物 `version` 仍为 `0.1.0+llm.qamock-model-v1`，**未借用** | ✅ **吻合** |
| **AC-005** | canary 跑完 4 条路径，产物/捕获/源码 0 命中 | canary `LEAKCANARY_abc123` 覆盖成功路径 + 3 条报错路径（HTTP 500 / 连不上 / 模型返乱码）；扫描 **109 个文件**（15 产物 + 85 捕获 + 夹具 + 日志），命中 **2 处且均为「设置 canary 的驱动脚本自身」**，产物/捕获/源码 **0 命中** | ✅ **吻合** |
| **AC-006** | 抽取器零改动 | `git diff --numstat` 对三个只读文件为空 | ✅ **吻合** |
| **AC-007** | 只查退化、不查雅俗；给人工抽查清单 | 构造库中有 `order` 表 + 中文名「订单 Order」⇒ 精确命中 `tokens_equal_to_entity_key=["order"]`；「客户订单」**不**被误判 | ✅ **吻合**（交付物层面有缺口，见 §7-F3） |
| **AC-008** | `--out` 必填无默认；非交互；`--help` 完整；失败非零码 | 不给 `--out` ⇒ `EXIT=1` + 明示「必填且无默认值」；源码无 stdin/TTY/交互调用；`--help` 56 行覆盖输入/输出/增量语义/**环境变量名**，无 key 值 | ✅ **吻合** |
| **AC-009** | 质量不判、可用性保证 | 可用性义务（AC-001/002/003）实测成立，但 AC-002 有一处边界未兜住（§7-F2） | ⚠️ **基本吻合，带 §7-F2 的缺口** |

---

## 5 · 我自己做的变异证据三元组（纪律 G）

**纪律 G 合规声明**：每次变异均给 ①变异期二进制 sha256 ②patch sha256 ③运行条数与逐条结果；
**编译失败一律不计为执行**；三次变异均**先验证运行时行为确实改变**（排除空变异），**回滚后重建的二进制与干净基线逐字节相同**。

干净基线二进制 sha256：`1c22f42d7b3463ee7cb4a767501bd130507216802c2fc8684fac09af4980589b`
（我 `cargo build --bins` 自建；三次变异全部回滚后重建，**逐字节相同**，见 `mut/clean.binary.sha256`）

### M1 · 让 `tokenize` **副本**与 `view.rs` 原件漂移

| | 内容 |
| --- | --- |
| **① 二进制 sha256** | `be5a593ce05e9977c076d8bfbed0c546486cdebbddf94bce2c5276902cc3093a` |
| **② patch sha256** | `512664d1f4854f60ae5ad95bf663843842bdd3b204b05c9c5ab41bbda710a217`（`mut/m1_tokenize_diverge.patch`） |
| **变异内容** | 仅改副本 `src/bin/ontok-translate-names.rs` 的 `to_ascii_lowercase()` → `to_lowercase()`；`src/domain/view.rs` **未动**（禁改文件） |
| **③ 运行条数与逐条结果** | ①**该 bin 单测 12 条：12 passed / 0 failed**（逐条全 `... ok`）<br>②**全量 798 条：797 passed / 1 failed / 1 ignored**；唯一失败 `test_cli_registry_path_is_byte_identical_when_registry_says_the_same`，断言输出为 `连不上数据库：error communicating with database: Connection reset by peer (os error 104)` —— **DB 连接抖动，与本变异无关**<br>③**运行时行为对照**（同一 schema、同一 mock、同一输入）：干净版退化告警 **0 条**，M1 版 **2 条** ⇒ **变异非空** |
| **结论** | **副本已与原件漂移且行为可观测地改变，全仓 798 条测试无一发现。** L-5 的「无任何测试覆盖」**经实验证实** |

### M2 · 让 `run()` 静默删除「schema 里已消失」的表

| | 内容 |
| --- | --- |
| **① 二进制 sha256** | `72f474cac0a8ba08dac41942035cbfab7dd5bd3d8c5d7e04a73fc02d5512d831` |
| **② patch sha256** | `ecbbe30ca12038981ca5032261909aefb1f5357e657b143cf1afb32e06b8062e`（`mut/m2_silent_delete.patch`） |
| **变异内容** | 在 `merge_names(...)` 之后加一行 `filter`，把不在 `schema_tables` 里的条目直接丢掉 |
| **③ 运行条数与逐条结果** | ①**该 bin 单测 12 条：12 passed / 0 failed** —— **全部漏检**<br>②**我自己的 AC-003 场景**：第 2 遍产物 `tbl_invoice` **从 names 里消失**、`absent_from_schema` 变空、退出码仍为 `0` ⇒ **我的断言检出该违规** |
| **结论** | 我的 AC-003 验证**非空**（能抓违规）；但**交付的 12 条单测对这个真实违规全绿** ⇒ 见 §7-F5 |

### M3 · 把 `agent` 换成硬编码常量（AC-004 自证）

| | 内容 |
| --- | --- |
| **① 二进制 sha256** | `2ecbe50fdd9d387dc9120a60842a5c790360b567cf9b183f6ce35d771a67892a` |
| **② patch sha256** | `99896460cac3c9b631ce7e100ef7c0ff1a9a4e8894dcb096f6311188d81569c7`（`mut/m3_hardcoded_agent.patch`） |
| **变异内容** | `module_path!()` → `"HARDCODED_LITERAL_NOT_SELF_REPORTED"` |
| **③ 运行条数与逐条结果** | 1 次运行：`EXIT=0`，产物 `agent` = `bin:HARDCODED_LITERAL_NOT_SELF_REPORTED`，`_coverage.generator` 同值；对照干净版 = `bin:ontok_translate_names` |
| **结论** | ✅ AC-004 要求的「换成硬编码就立刻看得出」**成立**；当前 `agent` 确实源自编译进来的模块路径，不是巧合 |

---

## 6 · 我独立发现的问题（按严重度）

### F2 · **中** · 退出码契约在「缺口=100%」时仍然重载，且 AC-002 的两处覆盖率出口同时消失

* **触发条件**：模型对**全部**表都没给出可用译名（我实测的 `mode=none`、以及「模型返回非 JSON 乱码」同路径）。
* **观测**：

  | 场景 | 退出码 | stdout 汇总行 | `_coverage` 缺口清单 |
  | --- | ---: | --- | --- |
  | 缺 `--schema`（命令错） | **1** | 无 | 无 |
  | 缺 `--out`（命令错） | **1** | 无 | 无 |
  | **缺口 = 100%（命令完全正确）** | **1** ← | **无** ← | **无（产物不落盘）** ← |
  | 缺口 = 2/5（命令完全正确） | 2 | `translated 3 / total 5` | 不落盘（设计如此） |

  错误信息均为：`错误：产物未通过 TableNames::load 的校验（**未写出任何文件**）: 翻译表是空的`

* **根因（源码定位）**：`TableNames::load(&text)` 在 `src/bin/ontok-translate-names.rs:650`，
  而缺口闸 `if !untranslated.is_empty() && !args.allow_gaps` 在 `:663`。
  100% 缺口时 `merged` 为空 ⇒ `load` 先 bail ⇒ **缺口闸永远走不到**。
  且汇总行 `println!("translated …")` 在 `:656`，位于 bail 之后 ⇒ 一并被短路。
* **影响**：a) 「改命令(1)」与「数据不完整」在 100% 缺口时**只凭退出码不可区分** ——
  恰是 AC-002 立这条 AC 的原因；b) AC-002 要求「stdout 汇总 **和** 机器可读缺口清单」两处都有，
  在这条路径上**两处同时缺席**，人只能从 stderr 的「翻译表是空的」猜。
* **不构成安全问题**：`load` 的 fail-closed 生效，**坏产物一律不落盘**，我已逐条验证。
* **严重度定为「中」而非「高」**：只在「一条都没翻出来」这一退化场景触发；真实模型给部分译名时行为完全正确。

### F1 · **低** · AC-001「中文名 = 全空格」这一行的期望与实测措辞不符

* **AC-001 期望表原文**：「中文名 = `"   "` ⇒ bail，**错误含该表名**」。
* **实测**：从**输出路径**注入全空格 ⇒ 退出码 1、不落盘（安全属性成立），但错误是
  `翻译表是空的`，**不含表名**。
* **根因**：`parse_translations` 在 `:449-451` 先 `cn.trim()` 并丢弃空串 ⇒ 全空格名在进入
  `TableNames::load` 之前就被上游滤掉，`merged` 变空，落到「names 非空」那条校验。
  即生成器比 `load` **更严格**，坏产物根本到不了 `load` 的第 2 条校验。
* **三条校验本身都真被强制执行**：我另外用 `--names` 喂入含全空格条目的已有表，
  实测 `错误：已有翻译表 … 未通过 TableNames::load 的校验: 表 tbl_order 的中文名为空` ——
  **确实指名道姓**。
* **判定**：安全属性（拒 + 不落盘）100% 成立；仅「错误归属路径」与 AC 措辞不符。归为**措辞偏差**，非缺陷。

### F3 · **低** · AC-007「人工抽查清单」与「mock 译文」标注**没有任何持久化载体**

* AC-007 要求交付「表名 + 中文名 + **列注释摘要**」的人工可抽查清单。
* 全仓检索 `"译文来自 mock"` / `"mock 翻译"` / `"仅证明"`：**0 命中**
  （范围含 `.review-workspace/` 全部 12 个相关文件，纪律 E 已先跑阳性对照确认扫描器能穿透该被忽略目录）。
* 产物 JSON 只含 `names`（表名→中文名），**不含列注释摘要**；列注释只存在于输入的 schema JSON 里。
* 代码里**有**等价的诚实声明（`:722-724`）：
  `注：本次译文**未经人工确认**。质量只能人看（AC-007 / 限制 L-1）`
  —— 这条会随每次运行打到 stderr，等价覆盖了 L-1，但**不等于** AC-007 要的那份清单。
* **判定**：AC-007 的**交付义务**（清单 + mock 来源标注）只存在于交付对话，**未入库、未落盘**。
  若 BA 要求可追溯，需要补一份落盘清单。

### F5 · **中** · 12 条单测全部只测纯函数，**`run()` 的接线没有任何测试覆盖**

这是 briefing 问的「还有没有别的『看起来测了、其实没测』的地方」的答案，**有**：

* M2 证据：在 `run()` 里加一行静默删除，**12 条单测全绿**，而产物已经违反 AC-003 最重的一条。
* 根因：AC-003 的三条单测（`plan_translates_only_missing_tables…`、`merge_keeps_entries_absent_from_schema`、
  `second_run_leaves_earlier_translations_byte_identical`）分别测 `plan_translations` / `merge_names`
  这两个**纯函数**；但真正决定「谁被写进文件」的是 `run()` 里的
  `merge_names` → `coverage` → `TableNames::load` → `std::fs::write` 这段**接线**，**无任何测试**。
* 后果可见：**F2 那个退出码缺陷就住在 `run()` 里**，纯函数层的 12 条测试结构上抓不到它。
* 建议（不在本次范围内实施）：把「两遍增量 + 产物落盘」做成一个端到端测试，或至少对
  `run()` 的退出码决策做一条集成断言。

### F6 · **低** · 退化自检命中时**仍返回退出码 0**

* 实测：`_coverage.degenerate_tokens` 命中 2 条（`order`/`customer` 表，中文名「订单 Order」），`EXIT=0`。
* 该名字能过 `TableNames::load`（「订单 Order」≠ 表名），但会被 `usable_as_human_name`
  按「词元相等」判为退化 ⇒ 下游**整个对象静默消失**——正是 AC-002 引用 `propose.rs:369-374` 的那个坑。
* 即：工具**准确检测出了会造成静默丢失的条件，却仍报成功**。
* **未见文档把它登记为限制**。建议 BA 拍板：退化是否也应当作「需人过目」（退出码 3）或登记为限制。

### F4 · **极低** · 行数声称 963，实测 964

`git diff --numstat` = `964  0`。声称 963。差 1 行，不影响任何判定，记录备查。

### F7 · **低** · 套件内存在一条 DB 抖动型不稳定测试

* M1 变异期全量中，`tests/test_extraction_agent.rs::test_cli_registry_path_is_byte_identical_when_registry_says_the_same`
  失败于 `Connection reset by peer (os error 104)`；同一条测试在干净全量中通过。
* 与本 REQ 无关（该文件零改动），但意味着**全量绿/红不是完全确定的**，
  「全量 0 失败」这一结论带一个已知的非本 REQ 抖动源。记录备查。

---

## 7 · 退出码设计是否站得住

**开发方的理由（原文）**：「文案会被改，把机器契约挂在散文上，一旦有人润色报错措辞脚本就静默走错分支；
拆成独立码后契约落在类型层面，单测能断言四码两两不同。」

**我的判断：理由方向站得住，但实现没有兑现它宣称的那条保证。**

* **理由本身**：成立。退出码是机器契约，不该由散文措辞决定 —— 这正是本项目一贯的纪律。
* **那条「断言四码两两不同」的��测确实存在**，在
  `src/bin/ontok-translate-names.rs:869-886` 的 `exit_codes_separate_command_error_from_data_incompleteness`，
  第 880-885 行：

  ```rust
  let all = [EXIT_OK, EXIT_COMMAND_ERROR, EXIT_GAPS_NOT_ACKNOWLEDGED, EXIT_REVIEW_NEEDED];
  for (i, a) in all.iter().enumerate() {
      for (j, b) in all.iter().enumerate() {
          assert!(i == j || a != b, "退出码 {a} 与 {b} 重载了");
      }
  }
  ```

* **但它断言的是什么**：**四个 `const` 常量彼此不相等**。
  它**没有**断言 `run()` 会返回哪个码、**没有**断言 `main()` 会把它交给 `std::process::exit`、
  **更没有**断言「有缺口时真的落到 2 而不是 1」。
  换言之：**它锁的是四个数字，而不是「语义不重载」这件事本身。**
* **F2 正是这条测试抓不到的地方**：100% 缺口时实际返回 1，与「命令错」的 1 重载，
  而四个常量本身确实两两不同 ⇒ **该测试仍然全绿**。
* 补充：开发方说「契约落在类型层面」——实际上这些是四个 `i32` 常量（`:75-88`），
  `coverage_exit` 的返回类型也是 `i32`，**类型层面并没有做任何区分**。

**结论：退出码方案（0/1/2/3 四分）设计正确、实现基本正确，但「互不重载」这一保证目前只由一条
纯常量测试支撑，实际存在一个未被覆盖的重载边界（F2）。**

---

## 8 · 已知限制是否被诚实登记

| # | 开发方登记内容 | 我的独立判定 |
| --- | --- | --- |
| **L-1** | 翻译质量无自动校验，只能人看 | ✅ **诚实**。代码 `:722-724` 每次运行都向 stderr 打「未经人工确认…质量只能人看（AC-007 / 限制 L-1）」；产物与运行时都不假装质量可判。 |
| **L-2** | `TableNames::load` 任一条失败是全表报废 | ✅ **诚实**。我在 `:19-25` 的文件头注释与 `:650` 的 fail-closed 调用两处都确认了它如实登记并照此实现。 |
| **L-3** | `propose.rs` 的静默过滤本 REQ 不改 | ✅ **诚实**。`propose.rs` diff 为空（已证），而生成器把覆盖率做成显式出口绕开它 —— 与登记一致。 |
| **L-4** | `_coverage` 被 `ontok-propose` 加载后丢弃 | ✅ **诚实且我实测确认**。文件头 `:49-51` 与 `:230-231`（`--help`）都写明；更关键：**我用真实消费方验证过 —— 带 `_coverage` 的好产物被 `ontok-propose` 正常接受（`EXIT=0`，翻译覆盖 4/4）**，说明「能反序列化但消费方拿不到」的描述准确。 |
| **L-5** | `tokenize` 副本与原件的**相等性无任何测试覆盖**；单测钉的是副本自己 | ✅ **改口诚实，且经实验证实**（详见下） |

### L-5 专项裁定：**诚实，是未封堵的已知限制，不是已解决**

我的独立取证链：

1. **原件确实是私有函数**：全仓 `tokenize` 定义只有两处 ——
   `src/domain/view.rs:636`（`fn tokenize`，**私有**，被 `view.rs:664` 使用）
   与 `src/bin/ontok-translate-names.rs:351`（副本）。`view.rs` 属红线只读 ⇒ 「物理上无法复用原件」**属实**。
2. **两份此刻逐字节相同**：对 `:637-640` 与 `:352-355` 做 `diff`，**零差异**（`BODY_IDENTICAL`）。
3. **那条单测断言的确实只是副本自己**：`tokenize_keeps_han_runs_whole`（`:912-921`）
   调用的是无限定名 `tokenize`，在 `use super::*` 下解析到**本文件的副本**。
   它的三条断言（`客户订单`、`订单 Order`、`robot_device`）**全部只涉及副本**，没有任何一条与 `view.rs` 比对。
4. **实验判定（M1）**：把副本改成与原件行为**可观测地不同**（`to_lowercase`），
   在输入含 U+212A KELVIN SIGN、中文名「Kelvin 温度」、库中有 `kelvin` 表时 ——
   干净版退化告警 **0 条**，M1 版 **2 条**（行为确实改变，排除空变异）。
   **而全仓 798 条测试（含该 bin 的 12 条单测）无一发现这次漂移。**
5. **该单测的文档注释本身就写着这句限制**（`:909-911`）：
   「⛔ 这条只钉**副本自己**的规则，**不**证明它与 `src/domain/view.rs` 的原件相等」。

⇒ **L-5 的措辞与事实完全一致。第一轮把它写成「已用单测钉住规则」是夸大；改口后是准确的。**
开发方在此处的自我修正是本项目里少见的**主动降级声明**，应予肯定。

### 「tokenize 漂移的后果边界」——我独立验证：**边界成立**

开发方称漂移「只影响退化自检的告警，不影响 `TableNames::load` 的校验」。

* `TableNames::load` 是 `src/extract/names.rs:44-58` 的**外部函数**，生成器在 `:650` **直接调用**它，
  与副本 `tokenize` **无任何数据依赖**。
* 副本 `tokenize` 在整个二进制里**只有一个调用点**：`:366`（`degenerate_tokens` 内），
  其产物只流向 `_coverage.degenerate_tokens` 与 stderr 告警。
* 合并路径（`merge_names`）、解析路径（`parse_translations`）、校验路径（`load`）**都不碰它**。

⇒ **边界成立，且是结构性成立的**（不是「恰好没影响」）。漂移最坏后果是退化自检漏报/误报一条告警，
不会让坏产物通过 `load`。M1 的实验结果也与之一致：只有退化告警数从 0 变 2，产物本身照旧通过 `load`。

### 其他「看起来测了、其实没测」的地方

见 **F5**：AC-003 的三条单测只覆盖 `plan_translations` / `merge_names` 两个纯函数，
`run()` 的**接线**（合并 → 覆盖 → 校验 → 落盘 → 退出码）**零测试覆盖**，
M2 证明真实违规能 12/12 全绿通过。**这是与 L-5 同类、但尚未被登记的第二个缺口。**

---

## 9 · 有没有夸大

| 声称 | 我的判定 | 依据 |
| --- | --- | --- |
| 「唯一新增一个文件，抽取器零改动」 | ✅ **属实** | `git diff --name-only` 仅 1 个文件；三个只读文件 numstat 为空 |
| 「三条只读文件 numstat 为空」 | ✅ **属实** | 已独立复核 |
| 「795 passed / 3 failed / 1 ignored」 | ⚠️ **数字属实，但归因错误** | 795+3=798 与我实测总数吻合；但那 3 条失败**不是** pre-existing 缺陷，而是 skip-worktree 位未重放。**要求 BA 选三条路之一是错的**（见 §10） |
| 「1 条瞬态：共享库项目撞名」 | ⚠️ **未复现，但我确实见到另一条瞬态失败** | 我没见到 `t_prop_{pid}_{n}` 撞名；但我在 M1 期见到 `Connection reset by peer`（F7）。「存在瞬态 DB 失败」这一**类别**属实，具体归因未验证 |
| 「四码互不重载」 | ⚠️ **部分不实** | 四个常量确实两两不同（单测为证），但 **100% 缺口时实际返回 1**，与命令错的 1 重载（F2） |
| 「canary 跑完 4 条路径，180 个文件 0 命中」 | ✅ **方向属实，口径我无法核对** | 我自己跑：109 个文件、0 命中（2 处命中均为设置 canary 的驱动脚本自身）。180 这个具体数字取决于其扫描范围，我**未独立复核**，不判真伪 |
| 「12 条单测纯函数层、零联网零花钱」 | ✅ **属实** | 逐条读完 12 条：只调 `plan_translations` / `merge_names` / `coverage` / `coverage_exit` / `tokenize` / `degenerate_tokens` / `entity_keys` / `parse_translations` / `TableNames::load`。**没有任何一条构造 `LlmTransport`**，而 `translate()` 只在 `run()` 里被调用、且需要注入传输层。实测该 suite 耗时 `0.00s` |
| 「L-5 无任何测试覆盖」 | ✅ **属实且我实验证实** | M1 全三元组，见 §5 / §8 |
| 「tokenize 漂移不影响 `load` 的校验」 | ✅ **属实** | 结构性验证，见 §8 |
| 「963 行」 | ⚠️ **差 1 行** | 实测 964 行新增 |

### 关于「AC-007 译文来自 mock」的标注

briefing 要求确认该标注**没有被去掉**。我的结论比「有没有去掉」更严格：

* 该标注在**任何持久化载体里都不存在** —— 全仓检索 0 命中（F3，纪律 E 阳性对照已验证扫描器有效）。
* 它只可能存在于交付对话中。**从可审计角度看，它等同于「已被去掉」**：仓库里既没有标注，也没有那份清单。
* 唯一幸存的等价物是运行时 stderr 的一句 `注：本次译文未经人工确认…（限制 L-1）`，
  它覆盖 L-1，但**不等于** AC-007 要求的那份「表名 + 中文名 + 列注释摘要」清单。

---

## 10 · 我推翻的地方

### 10.1 ✅ **推翻开发方：那 3 条失败不是「pre-existing 缺陷」，是 skip-worktree 位未重放**

* **开发方原话**：2 条判为「pre-existing：真实 schema 配不上占位翻译表」，并**要求 BA 在三条路里选一条**。
* **我的实测**（在位已重放的前提成立之后）：

  ```
  tests/drop_rate.rs::real_schema_drop_rate_report .................. ok
  tests/test_fk_naming.rs::test_real_schema_customer_id_has_exactly_one_display_name ... ok
  ```
  全量 **798 passed / 0 failed / 1 ignored**。

* **算术交叉验证**：开发方 795+3=798 = 我实测 798。两次独立运行**可运行测试总数一致**，
  说明「失败」与「通过」是同一批测试在不同 skip-worktree 位下的两种结果，
  而**不是**「多了 3 个真缺陷」。
* **机理独立验证**：入库版 `1.3.0-desensitized` 的 91 个表名（`t001…t091`）与真版 `1.2.0`
  的 91 个真实表名**零重叠** ⇒ 覆盖率 0% ⇒ 丢弃率飙升 ⇒ 正是这两条测试的判据。
* **对 briefing 前提的独立复核**：我没有因为 briefing 这么说就照单全收。我先验了三条前提
  （`S` 位、干净工作区、`1.2.0`/91），再**亲自跑了那两条红线测试**，才确认成因是位未重放。
* **裁定**：**briefing 的判定成立，开发方的归因与「要求 BA 选路」是错的。**
  正确处置是**重放 skip-worktree 位**（已由 briefing 完成），无需 BA 在三条路中做任何选择。
  开发方把一个**环境位问题**升级成了**需求级决策**，属于误判。

### 10.2 ✅ **推翻「四码互不重载」这一声称**（部分）

见 F2 与 §7：100% 缺口 ⇒ 实际返回 1，与「命令错」的 1 重载；
且那条「断言四码两两不同」的单测锁的是**常量**，不是语义，所以抓不到。

### 10.3 ✅ **推翻开发方给出的「AC-001 三种注入各自错误含表名」**

全空格注入的实测错误是「翻译表是空的」，不含表名（F1）。
**但**我同时证明三条校验都真被强制执行（用 `--names` 输入路径实测到「表 tbl_order 的中文名为空」）。
⇒ 推翻的是**措辞**，不是安全属性。

### 10.4 ⚠️ **对 BA 侧的一处提示（不是推翻）**

* `acceptance.md:262` 的基线 780 与实测 798 差 6（§3.4）。差额**不归本 REQ**，但基线数字应核对。
* `acceptance.md` 的 L-1…L-4 四条限制里，**没有 L-5** —— L-5 是开发方自己加的。
  这说明 BA 的限制清单**不完整**，开发方的补充是净收益，值得肯定。

---

## 11 · 未独立复核 / 受限项（诚实列出）

| 项 | 为什么没验 |
| --- | --- |
| develop `958f442` 的基线全量数字（780/0/1/59） | 需切换 worktree 分支，会改动工作区，超出「只验证不改」边界。故 §3.4 的 +6 差额只登记不归因 |
| 开发方声称「扫 180 个文件」的确切口径 | 其扫描范围/排除规则未交付，我无法复现该分母。我用自己的口径（109 文件）重做并得 0 命中 |
| 开发方声称的「瞬态：共享库项目撞名 `t_prop_{pid}_{n}`」 | 我未复现该具体现象。但同类 DB 瞬态失败我确实见到一条（F7） |
| AC-007 人工抽查清单的**内容质量** | 该清单未落盘（F3），无从审阅 |

---

## 12 · 给 BA 的处置建议（不在本次实施）

1. **必做**：修 F2 —— 把 `TableNames::load` 的调用**下移**到缺口闸之后（或在 100% 缺口时先走缺口闸），
   使「命令错(1)」与「数据不完整(2)」在任何缺口比例下都只凭退出码可分，并让 stdout 汇总行始终出现。
2. **应做**：给 `run()` 的接线补一条端到端测试（F5），否则同类缺陷还会再溜进来。
3. **待拍板**：退化自检命中时是否改为退出码 3（F6）。
4. **待拍板**：AC-007 清单是否要求落盘（F3）。
5. **保留**：L-5 作为已知限制登记，**不得**在后续 PR 中被写成「已封堵」。
   若要封堵，需 BA 授权改 `view.rs`（把 `tokenize` 提为 `pub(crate)`）或加一条跨实现一致性守卫。
6. **无需处置**：那 3 条「失败」已随 skip-worktree 位重放而消失，**不要**为它们开需求。

---

## 附 · 验证产物清单（`.review-workspace/req090qa/`）

| 文件 | 内容 |
| --- | --- |
| `mock_llm.py` | **我自己搭的** mock LLM 端点（8 种模式，含 `SKIP_TABLES` 参数化、请求表名落盘） |
| `mkfix.py` / `schema_a…f.json` | 我造的 6 份 schema 夹具（全人造表名，不含任何真实客户数据） |
| `run_mock.sh` | 启 mock → 跑生成器 → 记录退出码/stdout/stderr/请求日志 |
| `full-test.log` | 干净全量完整日志（1232 行，60 个 suite） |
| `m1-fulltest.log` | M1 变异期全量完整日志 |
| `out/`（24 个文件） | 全部产物 JSON（含手造的 3 份坏产物） |
| `caps/`（117 个文件） | 全部 stdout/stderr/退出码/mock 请求日志 |
| `mut/*.patch` + `*.binary.sha256` | 三组变异的 patch 与变异期二进制 sha256 |
| `help.out` / `help.err` | AC-008 的 `--help` 原文与流归属证据 |

**纪律声明**：
* 未改任何交付物：验证结束时 `git status --porcelain -uall` = 0 行，HEAD 未变，
  回滚重建的二进制 sha256 与干净基线逐字节相同。
* 未碰红线文件：`tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs` 全程零改动。
* 未删测试、未加 `#[ignore]`、未放宽断言。
* 临时文件只在 `.review-workspace/req090qa/`；未用 `/tmp`，未用 home。
* 未 push（仓库无 remote）；未 squash、未 force、未改历史。
* 冷编译纪律：未设 `CARGO_TARGET_DIR`、未用 `--release`、未建 probe crate、未 `ln -s`。
* 口令只经环境变量，未 echo 连接串，未写入任何文件或日志。
* 所有「零命中」均先跑阳性对照验证扫描器有效性（纪律 E）；所有路径基准均实地确认（纪律 F）。
