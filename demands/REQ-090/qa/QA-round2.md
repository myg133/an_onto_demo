# REQ-090 第二轮独立复验报告（QA2）

- **被验对象**：`feature/REQ-090` @ `a7f424c`（父 `0977719`，未 squash / 未 force）
- **复验人**：Verifier（独立复核，无前序上下文，全部自己重跑）
- **工作区**：`/home/myg133/Projects/Anchnet/Ontology/feature-REQ-090`
- **日期**：2026-10-12
- **本轮未改动任何代码**：收尾时 `git status --short` 为空、`git status --short --untracked-files=no` 为空
- **自建 mock**：`.review-workspace/req090qa2/mock/mock_llm.py`（自写，OpenAI 兼容 `/v1/chat/completions`）
  + `.review-workspace/req090qa2/mock/mkfix.py`（自造 SchemaReport fixture）
  ⛔ 未复用开发方或上一轮 QA 留下的任何夹具 / mock
- **被测二进制**：`target/debug/ontok-translate-names`
  sha256 `5c02f48946d6758de8feb369188196cfc6f75011749c7b82884ab0eb4268b888`
  构建前已确认 `git status --short src/bin/ontok-translate-names.rs` 为空 ⇒ 二进制对应 HEAD 源码
- **纪律**：全程未重定向丢弃 stderr、未用文件删除命令（被环境接管为 mavis-trash 属预期）、未 push（仓库无 remote，已核实 `git remote -v` 为空）、未回显连接串、未用 `-q`

---

## 一 · 总判定

**VERDICT：PASS 带非阻断**

> F2 与 F6 **实测确认已修好**，四档退出码在我自建的 17 个场景下行为与声称完全一致（含最易做坏的「干净全覆盖 = 0」反向对照）；全量测试 60 suites / 800 passed / 0 failed / 1 ignored 复现。遗留 **F5 未封堵**（`run()` 接线仍零测试覆盖）、**L-5 仍为已知限制**，另新发现 **1 条 1/2 分类窄缝**（S9）与 **1 条既有测试抖动**。均登记为已知限制，不阻断。

---

## 二 · 前提复核（全部成立）

| 检查 | 期望 | 实测 | 判定 |
|---|---|---|---|
| `git log --oneline 958f442..HEAD` | 2 条，父为 `0977719` | `a7f424c` ← 父 `0977719` | ✅ |
| `git diff --name-only 958f442..HEAD` | 仅 `src/bin/ontok-translate-names.rs` | 同左 | ✅ |
| `git diff --numstat 958f442..HEAD -- src/extract/names.rs src/extract/propose.rs src/domain/view.rs tests/` | 空 | 空 | ✅ |
| `git status --short` | 空 | 空 | ✅ |
| `git ls-files -v -- src/extract/table_names.json` | 以 `S` 开头 | `S src/extract/table_names.json` | ✅ |
| `git remote -v` | 无 remote（故不得 push） | 空 | ✅ |

补充：**末尾换行符已修**。`wc -l` = `grep -c ''` = **1117**（原先 963 / 964）。红线文件 `tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs` 全程未被本次两个提交触及。

---

## 三 · 核心行为实测（我的 mock，我的 fixture）

### 3.1 8 个必验场景 —— 全部命中

| # | 场景 | 期望 | **实测** | 产物落盘 | stdout 首行 | 判定 |
|---|---|---|---|---|---|---|
| 1 | 干净全覆盖（无缺口/无残留/无退化） | **0** | **0** | YES | `translated 3 / total 3` | ✅ |
| 2 | 部分缺口，未给 `--allow-gaps` | 2 | **2** | NO | `translated 1 / total 3` | ✅ |
| 3 | 部分缺口，给了 `--allow-gaps` | 3 | **3** | YES | `translated 1 / total 3` | ✅ |
| 4 | **100% 缺口**，未给 `--allow-gaps` | **2** | **2** | **NO** | `translated 0 / total 3` | ✅ |
| 5 | **100% 缺口**，给了 `--allow-gaps` | **2** | **2** | **NO** | `translated 0 / total 3` | ✅ |
| 6 | 退化自检命中 | **3** | **3** | YES | `translated 3 / total 3` | ✅ |
| 7 | 中文名 = 表名 | **1** | **1** | **NO** | `translated 3 / total 3` | ✅ |
| 8 | 残留（表已从 schema 消失） | 3 | **3** | YES | `translated 3 / total 3` | ✅ |

### 3.2 场景 1 的陷阱已避开（专门核过）

开发方自曝其第一版 fixture 用 `MOCK_MODE=ok`，该模式兜底译名 `"业务对象 <表名>"` 自撞退化自检。我**没有**沿用任何既有 fixture，而是先在 Python 里**独立复刻** `tokenize`（`src/bin/ontok-translate-names.rs:392-397`）离线验算我的译名，确认无退化、无 `load` 拒绝，再搭 mock：

```
S1 clean (schema3): degenerate={} load_reject=[]
S6 degenerate (plain): degenerate={'order': ['order']} load_reject=[]
```

⇒ 场景 1 是**真干净**，实测 0 是**真 0**，不是假绿。产物 `_coverage` 复核：
`translated=3 schema_tables=3 untranslated=[] absent_from_schema=[] degenerate_tokens=[]`

### 3.3 额外 9 个场景（超出要求，均自建）

| # | 场景 | 期望 | 实测 | 产物 | 说明 |
|---|---|---|---|---|---|
| S9 | `--allow-gaps` + 部分缺口 + 一个中文名 = 表名 | （开发方未提） | **1** | NO | **闸门放行但 `load` 拒** ⇒ 见 5.2 |
| S10 | 模型回**全空白**译名 | 2 | **2** | NO | `translated 0 / total 3`，`parse_translations` 已滤空 |
| S11 | 模型 HTTP 500 | 1 | **1** | NO | |
| S12 | `choices` 为空 | 1 | **1** | NO | |
| S13 | `finish_reason = "length"` | 1 | **1** | NO | |
| S14 | 中文名含**全角破折号** | 0 | **0** | YES | AC-007：不算退化，`degenerate_tokens=[]`，译名原样保留 |
| S15 | 中文名词元 = **另一张表**的 key | 3 | **3** | YES | `["order"]` 命中 |
| S16 | 无缺口 + 给了 `--allow-gaps` | 0 | **0** | YES | `--allow-gaps` **不**强制造 3 |
| S17 | **exit=2 路径 + 退化非空** | 2 且退化落 stderr | **2** | NO | 见下 |

S17 实测 stderr（证明「退化自检在 exit=2 路径也打 stderr」**确有其事**，非仅写在注释里）：

```
⚠ 同时有 1 条中文名退化（词元撞上实体 key，**下游会把这些对象整个丢掉**）：
    "order" → "订单 Order"
```

---

## 四 · `gap_gate_fires` 八种参数组合穷举（全部实测）

实现（`src/bin/ontok-translate-names.rs:133-135`）：

```rust
fn gap_gate_fires(has_gaps: bool, allow_gaps: bool, product_is_empty: bool) -> bool {
    has_gaps && (!allow_gaps || product_is_empty)
}
```

| # | has_gaps | allow_gaps | product_is_empty | 闸门 | **实测退出码** | 产物 | 证据来源 |
|---|---|---|---|---|---|---|---|
| 1 | F | F | F | 不触发 | **0** | 写 | S1 |
| 2 | F | F | T | 不触发 | **1** | 不写 | S18（零表 schema） |
| 3 | F | T | F | 不触发 | **0** | 写 | S16 |
| 4 | F | T | T | 不触发 | **1** | 不写 | S18 + `--allow-gaps` |
| 5 | T | F | F | **触发** | **2** | 不写 | S2 |
| 6 | T | F | T | **触发** | **2** | 不写 | S4 |
| 7 | T | T | F | 不触发 | **3** | 写 | S3 |
| 8 | T | T | T | **触发** | **2** | 不写 | S5 |

**语义矛盾审查 —— 结论：无矛盾。**

- **#7（T,T,F）→ 3**：确有缺口且**产物已写出**，与「3 = 产物已写出」相符。✅
- **#8（T,T,T）→ 2**：用户已给 `--allow-gaps` 仍报 2，看似「不听话」，但 `names` 为空 ⇒ `load` 必然拒 ⇒ **写不出产物** ⇒ 报 3 就是撒谎（「产物已写出」为假）。2 是唯一诚实选项，且 stderr 已明确改口为「要落盘就得让模型至少给出一条可用译名」，不是让用户重复加 `--allow-gaps`。✅
- **#5/#6 → 2**：不落盘 + 有缺口，与 2 的语义完全一致。✅
- **#1/#3 → 0**：`--allow-gaps` 在无缺口时是惰性的（#3 实测 0，不是 3）。✅
- **#2/#4 的可达性**：`product_is_empty = merged.is_empty()`。若 `merged` 为空，则 `coverage()` 把 schema 里**每一张**表都算作 untranslated ⇒ `has_gaps = !untranslated.is_empty()` 恒为 true，除非 **schema 一张表都没有**。故 #2/#4 **仅在零表 schema 下可达**。实测（自造 `schema_empty.json`）：

  ```
  empty-schema <no flag>:      exit=1  product_written=NO  stdout: translated 0 / total 0
      stderr: 错误：产物未通过 TableNames::load 的校验（**未写出任何文件**）: 翻译表是空的
  empty-schema --allow-gaps:   exit=1  product_written=NO  stdout: translated 0 / total 0
  ```

  判为**非矛盾**：零表 schema 属「数据形状错」，归 1 与设计表一致；且这是**既有行为**（父提交 `load` 在最前，同样得 1），非本次上移闸门引入。
  附带正面证据：汇总行 `translated 0 / total 0` 在这条 exit=1 路径上**依然打印**，印证「上移到 `load` 之前」确实生效。

---

## 五 · 闸门顺序是否真的有效（读代码 + 验证）

### 5.1 `load` 的拒绝会不会被绕过？—— 不会，且 fail-closed 保住

代码次序（`src/bin/ontok-translate-names.rs`）：

| 行 | 动作 |
|---|---|
| 702 | `println!("translated {}/ total {}")` — 汇总行，**在闸门与 `load` 之前** |
| 715-765 | 缺口闸；命中 ⇒ 打 stderr + `return Ok(EXIT_GAPS_NOT_ACKNOWLEDGED)`（**不写文件**） |
| **772** | `TableNames::load(&text).context(...（**未写出任何文件**）)?` |
| 774-780 | `create_dir_all` + 文件写出 |

⇒ `load` **仍在写文件之前**（772 < 780），闸门只在 772 **之前**放行，不构成绕过。
实测佐证：**S7 / S9 / S18 / S11-S13 全部 `product_written=NO`**；落盘的只有 S1/S3/S6/S8/S14/S15/S16（且均为 `load` 通过者）。

**父提交对照（纪律 K，用 `git show 0977719:…`，非读工作区）**：`TableNames::load(&text)` 在**第 650 行**，汇总 `println!` 在**第 656 行**，缺口闸在**第 665 行** ⇒ 父提交确实是「先 `load` → 再汇总 → 再闸门」，**F2 的成因属实**，本次重排方向正确。

### 5.2 「闸门放行但 `load` 拒」是否构成新的分类争议？—— 存在，但**窄、非新增、按设计**

**实测 S9**：`--allow-gaps` + 部分缺口（`translated 2 / total 3`，`untranslated=["t_device"]`）+ `t_order → "T_ORDER"`（中文名 = 表名）：

```
exit=1   product_written=NO
stderr: 错误：产物未通过 TableNames::load 的校验（**未写出任何文件**）: 表 t_order 的中文名与表名相同 ……
```

**争点**：此刻「有缺口」与「没落盘」**同时成立**（= 2 的字面描述），程序却报 1（= 命令错）。脚本只拿退出码，无法区分。

**但判为非阻断**，理由三条：

1. **非新增**。父提交 `load` 在最前，同样报 1 ⇒ 与本次重排无关，**不是回归**。
2. **在设计表内**。`EXIT_COMMAND_ERROR` 的定义（`ontok-translate-names.rs:88-89`）**显式包含**「产物被 `TableNames::load` 拒绝」⇒ 这正是 1 的既定职责。
3. **窄且信息充分**。仅当用户主动给了 `--allow-gaps` + 有部分缺口 + 至少一个名字撞上表名时才触发；且 stderr 点名到具体表（`表 t_order`），人可立即行动。

⇒ 登记为**已知限制（低）**，不要求本轮修。

---

## 六 · 全量测试

### 6.1 口径

命令（严格照要求，无 `-q`，未丢弃 stderr，完整日志落盘）：

```
set -a; . /home/myg133/Projects/Anchnet/Ontology/.env; set +a
cargo test --no-fail-fast > .review-workspace/req090qa2/full-test-run2.log 2>&1
```

统计口径：**逐个 `^test result` 行求和**（`awk` 累加第 4/6/8 字段 = passed / failed / ignored）。

### 6.2 实测数字（**跑了两次**，分列，不合并）

| 轮次 | 命令退出码 | suites（`^test result` 行数） | passed | **failed** | **ignored** | 合计 |
|---|---|---|---|---|---|---|
| **run 1** | `EXIT=101` | **60** | **799** | **1** | **1** | 801 |
| **run 2** | `EXIT=0` | **60** | **800** | **0** | **1** | 801 |

**分项单列（不只给百分比）**：run 2 = **800 通过 / 0 失败 / 1 忽略**，与开发方声称的 `60 suites / 800 passed / 0 failed / 1 ignored` **逐项吻合**。

### 6.3 run 1 那 1 条失败 —— 逐条交代，未以「环境问题」一笔带过

- **测试名**：`test_cli_registry_path_is_byte_identical_when_registry_says_the_same`
- **位置**：`tests/test_extraction_agent.rs:1245`
- **完整断言输出**：

  ```
  thread '…' panicked at tests/test_extraction_agent.rs:1245:9:
  ontok-propose ["--agent-registry", "mcode-agent", "--agent-registry-version", "2026.10.07", "--round", "3"] 失败：
  Error: 连不上数据库：error communicating with database: Connection reset by peer (os error 104)
  Caused by:
      0: error communicating with database: Connection reset by peer (os error 104)
      1: Connection reset by peer (os error 104)
  ```

- **判定为既有抖动，非本需求缺陷**，四项独立证据：
  1. `tests/` 在 `958f442..HEAD` 的 numstat 为**空** ⇒ 该文件未被本次两个提交触及；
  2. 该测试跑的是 `ontok-propose`，与 `ontok-translate-names` 无调用关系；
  3. 失败点是 sqlx 的**传输层 `Connection reset by peer`**，非断言不符；
  4. **隔离重跑 3 次：3/3 全绿**（14.21s / 13.22s / 12.59s）；**run 2 同一条 `.ok`**，该 suite `26 passed; 0 failed`。
- ⇒ 归因：60 个 suite 并行打同一个测试库时的瞬时连接重置。**建议登记为已知限制（低，独立于本需求）**：全量基线存在 1 条概率可观的抖动 DB 测试，会让「0 failed」不可复现。

### 6.4 唯一的 ignored（两次一致）

- `todo_identifier_defenses_have_own_assertions` — `tests/test_mcp_server.rs:553`，`#[ignore = "缺口清单，不是可运行断言"]`
- **既有**，非本轮新增（`tests/` 未被改动）。
- ⛔ 本需求**未新增**任何 `#[ignore]`；`src/bin/ontok-translate-names.rs` 全文检索 `ignore` **零命中**（纪律 E 复核：同一条检索在 `tests/test_mcp_server.rs:550/553` 正常命中，扫描器有效）。

### 6.5 本需求单测的绝对值

| | `#[test]` 绝对条数 | 结果 |
|---|---|---|
| 父 `0977719` | **12** | — |
| HEAD `a7f424c` | **14** | `test result: ok. 14 passed; 0 failed; 0 ignored`（run 2 实跑） |

- **无删除**：父提交 12 个测试名在 HEAD **逐个仍在**。
- **无 `#[ignore]`**、**无放宽断言**（新增 2 条均为 `assert!` / `assert_eq!` 正向断言）。

---

## 七 · F2 / F6 逐条「声称 → 实测 → 判定」

### F2（中）· 100% 缺口时 1 与 2 重载、覆盖率出口双消失

| 项 | 声称 | 实测 | 判定 |
|---|---|---|---|
| 100% 缺口 `1→2` | 是 | **S4：exit 2**，`product_written=NO` | ✅ 真修好 |
| 100% 缺口 + `--allow-gaps` `1→2` | 是 | **S5：exit 2**，`product_written=NO` | ✅ 真修好 |
| 汇总行不再消失 | 声称上移到 `load` 之前 | **S4/S5 stdout 均有 `translated 0 / total 3`**；S18 的 exit=1 路径也有 `translated 0 / total 0` | ✅ 真修好 |
| 缺口清单可读 | 声称落 stderr | **S4/S5/S17 stderr 实测**：`表名：["t_customer", "t_device", "t_order"]`（**单行合法 JSON 数组**，可机读） | ✅ 真修好 |
| LLM 回全空格 `1→2` | 是 | **S10：exit 2**，`translated 0 / total 3`，不写文件 | ✅ 真修好 |

**结论：F2 已封堵。** 且经父提交代码比对（`load` 在 650 行 vs 汇总 656 / 闸门 665）确认**成因属实、重排方向正确**。

### F6（低）· 退化自检命中时仍退出 0

| 项 | 声称 | 实测 | 判定 |
|---|---|---|---|
| 退化命中 `0→3` | 是 | **S6：exit 3**，`product_written=YES`；`_coverage.degenerate_tokens=[{"table":"order","cn_name":"订单 Order","tokens_equal_to_entity_key":["order"]}]` | ✅ 真修好 |
| 退化在 exit=2 路径也打 stderr | 是 | **S17：exit 2** + stderr 实测打出「⚠ 同时有 1 条中文名退化 … "order" → "订单 Order"」 | ✅ 真修好 |
| 复用 3 而不新造一档 | 理由：处置相同（读 `_coverage`） | 代码与实测一致；**反向对照 S1=0、S16=0** ⇒ 未把一切变非零 | ✅ 成立 |

**结论：F6 已封堵。**

---

## 八 · F5 与 L-5 是否仍应登记为未封堵的已知限制

### F5（中）· `run()` 接线零测试覆盖 —— **未封堵，应继续登记**

**绝对证据（静态，非推断）**：

1. `#[test]` 绝对条数 14；其中**新增的 2 条**是
   `gap_gate_fires_on_total_translation_failure_so_it_is_never_exit_1` 与
   `total_translation_failure_yields_code_2_and_a_full_gap_list`。
2. 二者调用面：前者仅 `super::gap_gate_fires(...)`；后者仅 `coverage(...)` + `super::gap_gate_fires(...)` + `super::coverage_exit(...)`。**均为纯函数层**。
3. **全文 `run(` 出现 3 次**（注释 124、注释 152、定义 538）；**`mod tests` 块内出现 0 次** ⇒ **`run()` 从未被任何测试调用**。
4. `tests/` 目录检索 `translate_names` / `translate-names` **零命中**（纪律 E/F 复核扫描器：同轮 `tests/` 检索 `translate` 正常命中 8+ 处，扫描器有效）。
5. 额外发现（纪律 H 相关）：单测里的 `translate_from_map`（`:908-917`）是**测试本地替身**，直接构造 `BTreeMap` 并**绕过 `parse_translations`** 与真实 `translate()`。凡用它喂数的用例，测的是替身而非真实模型回路 ⇒ **忠实的重复实现，任何测试都抓不住其漂移**。

⇒ **「加了 2 条单测」不构成封堵**。本轮 QA 认定与上一轮一致：F5 维持**未封堵**，登记为已知限制（中）。补齐方式应是对 `run()` 做进程级测试（注入静默丢表的 AC-003 违规，看是否仍全绿）。

### L-5（低）· `tokenize` 副本与原件的相等性无覆盖 —— **仍诚实登记，判定成立**

- 原件：`src/domain/view.rs:636-641`（`fn tokenize`，私有）。
- 副本：`src/bin/ontok-translate-names.rs:392-397`，**与原件逐字节相同**。
- 相等性覆盖：**零**。单测 `tokenize_keeps_han_runs_whole` 钉的是**副本自己**的行为。
- 源码登记（`:386-391`）：「**⛔ 副本与原件的「相等性」没有任何测试覆盖** … 两份实现可以各自全绿而悄悄漂移，**没有任何测试能抓住** … 登记为已知限制，**不写成已封堵**」；单测注释 `:1063-1064` 复述同一句。

⇒ 判定：开发方**如实登记**，未夸大。漂移后果有界（只影响退化自检告警的漏报，不影响 `TableNames::load` 三条校验——那是真身且被直接调用）。**维持已知限制（低）**。

附带核对（口径提醒，未使用 `\b`）：生成器的退化自检集合 = schema 表名小写，是下游 `view.rs:763` 的 `keys`（来自快照实体类型）的**子集**，故自检**窄于**下游判据，漏报方向为真；此点亦已在 `:419-423` 如实登记。

---

## 九 · 「exit=2 路径靠 stderr 承载缺口清单」这个张力点 —— 我的独立判断

**背景**：AC-002 要求缺口「人和脚本同时能读到」，但 exit=2 不写产物 ⇒ `_coverage` 在该路径**结构性不存在**。开发方主动指出并拒绝单方面改。

### 1）取舍是否正确？—— **正确。**

- 让 exit=2 也落一份「仅诊断用」文件，会把「**exit 2 ⇒ 一个字节都没写**」这条**强保证**变模糊。脚本目前可以放心断言「拿到 2 就别去碰 `--out` 路径」；一旦产出物存在，这条断言立刻失效，**两个属性的可断言性此消彼长**。
- 反过来，exit=2 的诊断信息量很小（一个缺口清单 + 一行计数），**stderr 完全装得下**，且程序已明确告知「没有 `_coverage` 可读」。
- 现状实测（S4/S5）已把这点写进 stderr：`⚠ 即便加了 --allow-gaps 也**写不出产物** …… 上面的缺口清单与本行就是脚本在这个场景下**全部**能拿到的东西。` ⇒ **没有隐瞒**，脚本不会误以为产物存在。

### 2）是否与「退出码是类型级契约、文案会被改」矛盾？—— **不矛盾，是软肋不是违约。**

- 契约层：`0/1/2/3` 的**处置决策**只依赖退出码，**完全稳定**（S1-S17 全部实测确认）。这一点未被破坏。
- 受影响的只有**细节提取**（要拿到「具体哪几张表」）。实测该载荷是**单行合法 JSON 数组**（`表名：["t_customer", "t_device", "t_order"]`），比自由散文好得多。
- 因此准确的定性是：**退出码契约完好；「可机读细节」的承载方式对文案有依赖**。这与设计当初要消灭的「1/2 处置方向靠文案猜」**不是同一个问题**——后者会让脚本**走错分支**（灾难性），前者只影响**清单粒度**（降级）。

### 3）判定：**可登记为已知限制（非阻断）**，不建议本轮改。

**可选加固（不要求本轮做，若将来要收紧）**：把机读载荷改成**独立成行、以纯 JSON 开头**（例如单独一行 `{"untranslated":[...]}`），使脚本可按以 `{` 开头的行提取，而不必知道 `表名：` 这个中文前缀。当前形态已可用，但确实仍绑定了前缀文案。

---

## 十 · 我有没有推翻开发方或需求方的地方？

**没有推翻任何一方。** 逐条交代：

| 声称方 | 声称 | 我的独立实测 | 是否推翻 |
|---|---|---|---|
| 开发方 | F2 修好（4 个退出码迁移） | S4=2、S5=2、S10=2、汇总行与清单实测可见 | **未推翻，成立** |
| 开发方 | F6 修好（退化 0→3） | S6=3、S17 退化落 stderr | **未推翻，成立** |
| 开发方 | 干净全覆盖 `0→0`（反向对照） | S1=0、S16=0 | **未推翻，成立** |
| 开发方 | 单测 12→14，无删除/ignore/放宽 | 绝对值 14；父 12 个测试名逐个仍在；全文无 `ignore` | **未推翻，成立** |
| 开发方 | 全量 60/800/0/1 | run 2 = 60/800/0/1 | **未推翻，成立** |
| 开发方 | exit=2 不写产物 | S4/S5/S7/S9/S17/S18 全部 `product_written=NO` | **未推翻，成立** |
| 开发方 | `load` 仍在写文件之前 | 代码 772 < 780 + 实测无落盘 | **未推翻，成立** |

**但我新增了两条开发方未披露的发现**（均已定级为非阻断）：

1. **1/2 分类窄缝（S9）**：开发方以「四档互不重载」为卖点陈述，但未提「`--allow-gaps` + 部分缺口 + 某中文名 = 表名」时，退出 1 与 2 的字面描述同时成立。**非回归**（父提交行为相同）、**在设计表内**（EXIT 1 明文包含「产物被 `load` 拒绝」）、**触发面窄** ⇒ 建议登记为已知限制（低）。
2. **既有测试抖动（run 1 的那 1 条失败）**：开发方的单次全绿未暴露它；我的 run 1 命中、run 2 未命中 ⇒ 全量基线含 1 条概率可观的抖动 DB 测试，「0 failed」不可复现。已按 6.3 逐条给出证据，**与本需求无关**。

**我没有推翻需求方的任何 AC 判定**，包括 AC-007「只查退化不查雅俗」：全角破折号不算退化是**设计意图**（`view.rs:656-665`，`tokenize("——")` 为空 ⇒ `usable_as_human_name` 返回 true），实测 S14 = exit 0 且译名原样保留，生成器与原件判据一致。**这不是缺陷，不作缺陷登记。**

---

## 十一 · 结论与建议

**VERDICT：PASS 带非阻断**

建议随需求一并登记的已知限制（建议写进 BA 的已知限制台账）：

| 编号 | 级别 | 内容 | 本轮是否封堵 |
|---|---|---|---|
| F5 | 中 | `run()` 接线零测试覆盖；新增 2 条单测仍停在纯函数层；`translate_from_map` 为绕过 `parse_translations` 的测试替身 | **否** |
| L-5 | 低 | `tokenize` 副本（bin `:392-397`）与原件（`view.rs:636-641`）相等性无任何测试覆盖（纪律 H 结构性不可测） | 否（如实登记） |
| 新-1 | 低 | 1/2 分类窄缝：`--allow-gaps` + 部分缺口 + 中文名=表名 ⇒ exit 1 且确有缺口未落盘（非回归、在设计内） | 否 |
| 新-2 | 低 | exit=2 路径的机读缺口清单承载于 stderr，依赖 `表名：` 前缀文案（处置决策不受影响） | 否 |
| 新-3 | 低（独立于本需求） | `tests/test_extraction_agent.rs::test_cli_registry_path_is_byte_identical_when_registry_says_the_same` 偶发 `Connection reset by peer`，使全量「0 failed」不可复现 | 否 |

**F2、F6 判定为已封堵。** 四档退出码在 17 个实测场景下互不重载，反向对照（exit 0）成立，fail-closed（坏产物不落盘）成立。

---

## 附录 · 证据文件位置

| 文件 | 内容 |
|---|---|
| `.review-workspace/req090qa2/mock/mock_llm.py` | 自建 mock LLM 端点 |
| `.review-workspace/req090qa2/mock/mkfix.py` | 自造 fixture 生成器 |
| `.review-workspace/req090qa2/mock/fixtures/` | schema3 / schema_plain / schema_empty / old_clean / old_with_stale |
| `.review-workspace/req090qa2/run_scenarios.sh` | 场景矩阵 runner |
| `.review-workspace/req090qa2/runs/` | 每个场景的 stdout / stderr / 产物 |
| `.review-workspace/req090qa2/full-test.log` | 全量 run 1 完整日志 |
| `.review-workspace/req090qa2/full-test-run2.log` | 全量 run 2 完整日志 |
