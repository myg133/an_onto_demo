# QA 验收报告 · REQ-049（verify/REQ-049 @ 7343bb0）

- 验收人：QA（Verifier 子会话）
- worktree：`/home/myg133/Projects/Anchnet/Ontology/verify-REQ-049`，分支 `verify/REQ-049`，HEAD `7343bb0`
- 基线：`9d6c529`（父提交）
- 无 `target` 复用、未建 probe crate、未 `ln -s`、未 `--release`

---

## 0. 结论

**PASS（带非阻断发现）**

- Dev 的反例**成立**，且比他自述的更强。
- 「重复表名 ⇒ 门禁会红」这个命题是**假的**（不是「分情况」）。
- 落 `warn` 不落 `bad` **判断正确**。
- 新提示**没有引入新的过度声称**，但有一处**上一轮就存在的过度声称在新句式下被放大**（`--out` 分支的「路径是它们实际会读的位置」）。
- 「实测过的假信心」这句引用的**运行时文案不含任何数字**，因此不存在 96.9% / 100.0% 的口径冲突；96.9% 只出现在文档注释里，我独立复现成功。

---

## 1. Dev 的反例：成立吗？

### 1.1 独立复现（全部由我重造产物）

造产物方式：以真产物 `.review-workspace/fix-props/schema.json` 为底，**只改表名、列与注释原样保留**，重算表头计数保持自洽。

| 产物构造 | 重复名数 | preflight | `drop_rate` | `fk_naming` |
|---|---|---|---|---|
| 真产物（基线） | 0 | EXIT=0 | **EXIT=0**，7.8%（889/964 存活） | EXIT=0 |
| 91 张**全**同名 → 改名成 `t001`（**CN 名可解析**） | 90 | EXIT=0 + 警告 | **EXIT=0，8.0%**（887/964） | EXIT=101 |
| 91 张**全**同名 → 改名成 `dup_table`（**CN 名查不到**） | 90 | EXIT=0 + 警告 | **EXIT=101，96.9%**（30/964 存活） | EXIT=101 |
| 91 张里**只有 2 张**同名（`t001` ×2） | 1 | EXIT=0 + 警告 | **EXIT=0，7.8%** | EXIT=0 |

**口径说明**：`表 N · 抽出属性 964 · 视图存活 K · 丢弃 D · 丢弃率 R%`，取自
`cargo test --test drop_rate -- --nocapture` 的自打印行；`EXIT` 为该次命令退出码
（`grep`/`PIPESTATUS` 复测，非依赖管道）。

### 1.2 命题真假：**假**

Dev 的命题「重复表名 ⇒ 门禁会红」被否证，而且否证得比他更强：

**只变重复比例、名字仍可解析**（全部改名成 `t001`）：

| 改名张数 k | 重复名数 | `drop_rate` 丢弃率 | EXIT |
|---|---|---|---|
| 1 | 0 | 7.8% | 0 |
| 2 | 1 | 7.8% | 0 |
| 5 | 4 | 7.8% | 0 |
| 10 | 9 | 7.8% | 0 |
| 20 | 19 | 7.8% | 0 |
| 45 | 44 | 7.9% | 0 |
| 91 | 90 | **8.0%** | **0** |

⇒ **重复名从 0 涨到 90（91 张表全部同名），`drop_rate` 全程绿。重复本身永远不会让 `drop_rate` 变红。**

**真凶是「改名」，不是「重名」**：改名会让表在 `table_names.json` 里查不到中文名，
`entity_display_name()`（`src/extract/propose.rs:511-519`）回落到表注释，而真产物的表注释是
`null` → 显示名为空 → `project()` 的 `usable_as_human_name()`（`src/domain/view.rs:656-665`）
把整个实体滤掉 → 存活属性塌陷。

**边界在哪**（改名到无 CN 映射的名字 `ghost_tbl`，只改名字）：

| 改名张数 k | 丢弃率 | EXIT |
|---|---|---|
| 1 | 8.8% | 0 |
| 2 | 9.4% | 0 |
| **3** | **9.4%** | **0（绿）** |
| **4** | **10.2%** | **101（红）** |
| 5 | 10.5% | 101 |
| 10 | 12.6% | 101 |
| 45 | 51.2% | 101 |
| 91 | 96.9% | 101 |

⇒ **边界就是 `drop_rate.rs:57` 的 `MAX_DROP_RATE = 0.10`**，与重复名数量**无关**。
注意 k=2 那格：已经存在 1 个重复名，**照样绿**；k=4 那格：重复名才 3 个，**已经红**。
重复计数在边界两侧都是「有的」，所以它不是判别量。

**`fk_naming` 也不简单**（重复名数扫描，名字仍可解析）：

| 重复名数 | 1 | 4 | 9 | 10 | 11 | 15 | 19 | 44 | 90 |
|---|---|---|---|---|---|---|---|---|---|
| `fk_naming` | 绿 | 绿 | 绿 | 绿 | **红** | 红 | 红 | 红 | 红 |

⇒ 它**确实**有一个重复数相关的边界，但机理不是「比例」而是**精确集合断言**：
`tests/test_fk_naming.rs:362` 断言 `plain == 33`，`:363-370` 断言 `qualified` **恰好**是
`t012` / `t013` / `t027` / `t032` 四条。
我的改名顺序按字母序，第 12 张表正好是 `t012` ⇒ 改名集合吞掉它的那一刻起红。
**这是「改名集合吞掉了哪张表」决定的，与重复比例无关。**

**所以完整答案**：两个门禁都不会因为「重名」变红；它们变红是因为**改名破坏了抽取器/投影器
依赖的语义输入**（CN 名映射丢失、FK 目标表消失、精确集合断言被打破）。

### 1.3 落 `warn` 对不对：**对**

- 我构造的 `sem_k091`（91 张全同名）`drop_rate` 是**绿**的。若落 `bad`，preflight 会拦掉一个
  门禁自己会放行的产物 —— 这正是「拿被反例推翻的断言去拦合法产物」。
- Dev 的「来源信号 ≠ 语义判断」论证我独立复核成立：`src/extract/introspect.rs:260`
  确为 `match tables.iter_mut().find(|t| t.name == tname)`，找到就 push 列、找不到才新建
  `TableMeta` ⇒ **同一次 `introspect()` 不可能吐出两个同名表**。重复表名确实是「这份产物不是
  这条路径产出的」的来源信号。

**建议（非阻断）**：warn 文案现在只说「重复表名 ≠ 门禁会红」，但没给出**真正的**判别量。
建议补一句「门禁红红往往是因为改名让实体丢了中文显示名」，否则读者仍会把重复数当判别量。

---

## 2. 「读得到 ≠ 会通过」这句的结构性论断

### 2.1 preflight 确实碰不到那三件事 —— 成立

`src/bin/ontok-gate-preflight.rs` 的全部 `use` 只有三条：

- `:61` `use anyhow::{anyhow, bail, Context, Result};`
- `:62` `use ontok::extract::introspect::{introspect, SchemaReport, SOURCE_DATABASE_URL_ENV};`
- `:63` `use std::path::{Component, Path, PathBuf};`

无 `extract`、无 `table_names`、无 `project`、无 `humanize_column`。**结构上碰不到**，
与 BA 的独立复核一致。

### 2.2 会不会腐烂：**会，且没有任何机制拦**

- `tests/` diff 为空 ⇒ **preflight 零测试覆盖**（`grep -rn "gate.preflight" tests/` 无命中）。
- 「本命令不跑 `extract()`」目前只是一句**注释 + 文案**，没有任何断言或编译期约束守着它。
  将来有人给 preflight 加一步抽取（哪怕只是为了多报点信息），这句话会**静默变成假话**，
  而全套 574 条测试**一条都不会红**。

**判断**：这句在**今天的代码事实上成立**，但属于「目前恰好不调用」，**无防腐烂机制**。
这是本轮我认为最值得后续补的一条（非阻断，因为当前陈述为真）。

---

## 3. 新提示逐句扫描

| 句子 | 判定 | 依据 |
|---|---|---|
| 「两个门禁都能**读到并解析**这份文件」 | 成立 | 两门禁同用 `ONTOK_SCHEMA_JSON` 优先 + 各自缺省路径（`drop_rate.rs:29`、`test_fk_naming.rs:312`） |
| 「格式是它们同一个 `SchemaReport`」 | **成立** | preflight `:62` 与 `drop_rate.rs:25`、`test_fk_naming.rs` 同取 `ontok::extract::introspect::SchemaReport`，是**同一个类型**，不是「看起来像」 |
| 「计数自洽」 | 成立 | `validate()` `:318-353` 校验 `tables.len()==table_count`、列数求和、各表自称列数/注释列求和 |
| 「体量在区间内」 | 成立（措辞略含糊） | 指 preflight **自己的**区间，不是门禁阈值；见下方出处问题 |
| **「路径是它们实际会读的位置」** | ⚠️ **过度声称（仅 `--out` 分支）** | 见下 |
| 「实测过的假信心：91 张同名表 + 完整列 ⇒ 本命令通过，drop_rate 门禁仍判红」 | **成立但欠限定** | 见下 |

### 3.1 「路径是它们实际会读的位置」—— 唯一的过度声称

`resolve_target()` 的三条分支待遇不同：

- **缺省分支**（`:206-222`）：真的做了核对 —— 把 `../.review-workspace/fix-props/schema.json`
  按 CWD 词法规整后与绝对字面量比对，不一致就 `bail!`。**这句话在这里站得住。**
- **环境变量分支**（`:200-204`）：也站得住 —— 两个门禁都读 `ONTOK_SCHEMA_JSON`。
- **`--out` 分支**（`:197-199`）：`return Ok(...)` **直接返回，一行核对都没有**。
  而 `--out` 是 help 里公开宣传的开关（`:569` 「`--out <path>` 产物落盘路径」）。

**实测反证**（`ONTOK_SCHEMA_JSON` 未设置）：

```
$ cargo run --bin ontok-gate-preflight -- --verify-only \
    --out <worktree>/.review-workspace/verify-req049/prods/sem_k091.json
  来源        ：--out 显式指定
前置检查通过（--verify-only）
  ✅ 两个门禁都能**读到并解析**这份文件：
EXIT=0
```

而两门禁的实际缺省位置是 `tests/drop_rate.rs:29`（crate root 的 `../`）与
`tests/test_fk_naming.rs:312`（**仓库根**的绝对路径），**都不指向 worktree 里的这个文件**。
⇒ 在 `--out` 分支，这句话是**假的**。

**归属**：`--out` 的早返回在基线 `9d6c529` **就已存在**（已 diff 确认两版 `resolve_target`
的该分支完全相同），所以**不是本轮引入的回归**。但「路径是它们实际会读的位置」这个**新句式**
把它从「没说清」升级成了「明确说了假话」，而这正是本会话连续栽跟头的同一类缺陷。
**严重度：中低**（仅影响显式 `--out` 且不同步设环境变量的用法），**建议非阻断修复**。

### 3.2 「体量在区间内」的下界出处

- 下界 80 表：**确实对齐** `tests/drop_rate.rs:64` 的 `MIN_TABLES = 80`，文档注释也这么写。
- 下界 900 列：数值上**恰好等于** `tests/drop_rate.rs:65` 的 `MIN_PROPS = 900`，
  但 preflight 的文档注释（`:79-80`）写的是「按真实产物标定（实测 1051 列 / 91 表），留约 15% 余量」，
  **只字未提 `MIN_PROPS`**。

⇒ 数值巧合正确，**出处标注失真**。若日后 `MIN_PROPS` 改成 950，preflight 的 900 会静默漂移，
而读注释的人不会想到要同步改它。**建议补一句「对齐 `drop_rate.rs:65` 的 `MIN_PROPS`」**（非阻断）。

### 3.3 「实测过的假信心」引的是哪个数字

- **运行时文案不含任何数字**（`print_ok` `:548`）⇒ 不存在 96.9% 与 100.0% 的口径冲突。
  Dev 与 QA 的数字不一致**没有**泄漏到用户可见输出。
- **文档注释**（`:512-514` 与文件头 `:47-49`）引 **96.9%**，并标注「2026-10-10 实测，人造产物」。
  我**独立复现出 96.9%**（`丢弃 934/964`，EXIT=101）—— 数字对得上。
- **但引用欠限定**：该 96.9% 只在「改成的名字**查不到** CN 映射」时成立。同样的
  「91 张全同名 + 完整列」构造，若改成 `t001`（可解析），`drop_rate` 是 **8.0% / EXIT=0 绿**。
  文档注释原文「把真产物 91 张表改名成同一个名字、列内容原样保留」没有点出这个前提条件。
- 另外 QA 报的 100.0% 与 Dev 的 96.9% 之差，机制上说得通：全丢是 `kept=0`（100.0%），
  而本构造里仍有一张表的实体活下来贡献了 30 条属性（96.9%）。**定性一致、数字不同，不构成冲突。**

⇒ 建议把这句改成点名因果（「改名导致实体在投影里被整体丢弃」），否则它**很容易被读成
「重名就会红」—— 恰好是本需求要消灭的那个误解**。

---

## 4. AC-003 四种情形：独立复现

| 情形 | 我的构造 | EXIT | 关键输出（实测） |
|---|---|---|---|
| 正常 | `--verify-only` + `ONTOK_SCHEMA_JSON` 指向真产物 | **0** | 「✅ 两个门禁都能**读到并解析**这份文件」 |
| 源库不可达 | `ONTOK_SOURCE_DATABASE_URL` 指向 `.invalid` 主机 + 哨兵口令 | **1** | 「Error: 源库不可达 / 勘查失败：连不上源库…: error communicating with database: failed to lookup address information: Name or service not known」+ 完整三选一指引；**产物未落盘** |
| 非合法 JSON | 写入 `{ this is not json` | **1** | 「产物不是合法的 schema JSON（门禁会在这里直接 panic）：…（key must be a string at line 1 column 3）」 |
| 表数偏少 | 真产物截到 50 张表（574 列） | **1** | 「表数异常偏少：50 张 < 下界 80（对齐 drop_rate.rs 的 MIN_TABLES）」+ 「列数异常偏少：574 列 < 下界 900」 |

四条全部复现，退出码与 Dev 自述一致。

### 4.1 口令哨兵测试方法是否可靠：**方法本身可靠，但覆盖面窄**

我用**假口令 `SENTINEL_PW_9f3a2b7c`** 试了 **6 种**会走到不同 sqlx 分支的连接串：

| 构造 | EXIT | 输出含哨兵？ | 库侧实际错误 |
|---|---|---|---|
| `.invalid` 主机 | 1 | 0 | `failed to lookup address information: Name or service not known` |
| 坏 scheme | 1 | 0 | `pool timed out while waiting for an open connection` |
| 非法端口 | 1 | 0 | `error with configuration: invalid port number` |
| 非法 `sslmode` | 1 | 0 | `error with configuration: unknown value "bogus" for ssl_mode` |
| 口令含在 user 段 | 1 | 0 | `pool timed out …` |
| 非 URL 串 | 1 | 0 | `relative URL without a base` ← 即任务书提到的那句 |

⇒ **6/6 都不回显口令**，任务书里「`relative URL without a base` = URL 取残缺，不是环境坏了」
得到印证（`EXIT=1`、报文清晰、可指导补救，是**产品行为正确**）。

**但**：这 6 条路径**没有一条**让 sqlx 把连接串写进错误文本 ⇒ **`sanitize()` 的打码分支
一次都没被触发**。Dev 的诚实限定**准确**。

**我对 `sanitize()` 本身的独立判断**（`:451-472`，纯静态走查，不需执行）：
逐 token 找 `://` → 取 authority → 找 `@` → 替换 userinfo。我手工推演了
`postgresql://user:secret@host:5432/db` → `postgresql://<userinfo-redacted>@host:5432/db` ✅；
无路径、无 `@`、口令前置在 user 段、以及 token 内嵌 `url=postgresql://…` 四种形态也都正确 ✅。
**逻辑本身是对的**，只是**没有任何测试或可触发路径证明它**。

### 4.2 Dev 的诚实限定够不够：**够，但要点明残余**

- 「本次只证明该情形不回显，没证明打码机制本身有效」—— **准确、不过度也不掩饰，可以接受。**
- 残余风险要说清：「口令绝不回显」这条保证**当前实际靠的是第二重**
  （sqlx 不把连接串写进 message），**而不是 `sanitize()` 这第一重**。若将来 sqlx 改行为，
  唯一兜底就是一段没被测过的代码。
- 他说「加测试会改条数所以没加」—— 这个理由**成立但不足以免掉**：`tests/` diff 为空正是
  「零测试覆盖」的成因。**建议后续补**（把 `sanitize()` 挪进 lib 或加 bin 内 `#[cfg(test)]` 单测，
  不影响 `cargo test` 的测试二进制条数）。**非阻断。**

---

## 5. 测试、环境、红线、产物 sha

### 5.1 全量（同会话对照）

DB 环境变量按任务书方式设置：
`export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' <repo>/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"`
（已核：末段为 `ontol`；全程未 echo 连接串，口令不入报告）

| 轮次 | 提交 | 结果 | 退出码 |
|---|---|---|---|
| HEAD 轮 | `7343bb0` | **574 / 0 / 1**（passed/failed/ignored），41 个 result block | 0 |
| **基线对照** | `9d6c529` | **573 / 1 / 1**，41 个 result block | **101** |

**口径**：用 `awk` 累加每行 `test result: ok. N passed; M failed; K ignored` 的第 4/6/8 字段
（**未**使用 `paste -sd+`）。原始日志：`.review-workspace/verify-req049/full-run1.log`、
`full-baseline.log`。

**运行期判据 vs 环境类失败，严格二分**：

- 基线那唯一一条失败是
  `tests/test_extract_cli_req019.rs:325` →
  `第 2 轮应当跑通，stderr：Error: 连不上数据库: error communicating with database: Connection reset by peer (os error 104)`。
- **该失败出现在「不带本次改动的基线」上** ⇒ 判定为**环境类**，与本次改动**无因果关系**。
- 它与 Dev 自述的第 1 轮 `573/1/1 (Connection reset by peer 环境类)` **同一现象**，
  独立得到复现（第二次观测）。
- **复现率随负载浮动，本报告不声称任何稳定复现率。**

**结论：本次改动零回归。** HEAD 轮 574/0/1，且 41 个 result block 与基线**同数**，
`tests/` 文件数 HEAD 与基线**同为 37**（`git ls-tree -r --name-only <rev> tests/ | wc -l`）。

**环境类限制（如实登记）**：本机**无 `psql`、无 `psycopg`** ⇒ `SELECT 1` 的 p50 **未测**。

### 5.2 红线核查

| 红线 | 结果 | 证据 |
|---|---|---|
| `tests/drop_rate.rs` 零改动 | ✅ | `git diff --stat 9d6c529 HEAD -- tests/drop_rate.rs` 为空 |
| `tests/test_fk_naming.rs` 零改动 | ✅ | 同上，为空 |
| `src/domain/view.rs` 零改动 | ✅ | 同上，为空 |
| `tests/` 整体零改动 | ✅ | `git diff 9d6c529 HEAD -- tests/` 为空；文件数 37 = 37 |
| 无测试被删除 | ✅ | 无删除文件；result block 数 41 = 41 |
| 无新增 `#[ignore]` | ✅ | 唯一 ignored 是 `todo_identifier_defenses_have_own_assertions`，其 attr 为**既有**，且注释自述「缺口清单，不是可运行断言」 |
| `load_and_verify()` 的 fail-closed 未削弱 | ✅ | 见下 |

**fail-closed 独立核实**：对比基线与 HEAD 的 `load_and_verify` —— HEAD 版本
（`:250-277`）与基线版本**错误路径逐条一致**：`read_to_string` → `map_err` 报错；
`serde_json::from_str` → `map_err` 报错；`bad` 非空 → `bail!`。
HEAD 仅把返回类型从 `Result<SchemaReport>` 改为 `Result<(SchemaReport, Vec<String>)>`
（新增 warn 通道），**没有新增任何「读不到就跳过」的分支**。
另：`ONTOK_SCHEMA_JSON` 分支（`:200-204`）也不构成绕过，因为两门禁都读同名环境变量。

### 5.3 产物 sha（**逐位对上**）

| 产物 | 期望 | 实测 | |
|---|---|---|---|
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | 同 | ✅ |
| `proposal.json`（`--project extracted`） | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | 同 | ✅ |
| 真产物 `schema.json`（**只读，未动**） | `b71c2afd…`（任务书给出前缀） | `b71c2afde6c103132e4e1ca6b9896aab442bd80abbe1fcc0870f35ddce0f2019` | ✅ |

**命令口径**（命令属实为 `ontok-propose`，`--out` 在 `--stats` 之前，**不传 `--names`**）：

```
cargo run --bin ontok-propose -- --schema <root>/.review-workspace/fix-props/schema.json \
    --out <D>/stats.json --stats <D>/stats.json      # 注意 --stats 自带路径参数
cargo run --bin ontok-propose -- --schema <root>/...schema.json \
    --out <D>/proposal_extracted.json --project extracted
```

**⚠️ 口径修正（值得记录）**：`--stats` **必须带一个路径参数**，
`--stats` 单独给会报 `Error: --stats 需要一个参数`（EXIT=1）。
首次按字面「`--out X --stats`」调用会失败；须写成 `--out <proposal> --stats <stats路径>`。

**真产物全程只读**：全程未对
`<repo>/.review-workspace/fix-props/schema.json` 执行写、删、改；所有 preflight 运行
一律用 `--verify-only` 或 `--out <verify 目录>`。结束前复验 sha 未变（上表）。
真产物头部复核：`table_count=91 / column_count=1051 / commented=681 / fk=0 / tro=on`，
`introspected_at=2026-10-05T04:25:53Z`。

---

## 6. 发现但**未修**的问题（均非阻断，按严重度排序）

1. **【中低 · 过度声称】`--out` 分支的「路径是它们实际会读的位置」为假**（§3.1）。
   `resolve_target():197-199` 无条件早返回，不核对两门禁是否会读该路径。
   建议：`--out` 分支也做一次「该路径是否等于两门禁解析后的缺省路径 / 环境变量」的核对，
   或把该句降级为「本命令校验的是**这份**文件，门禁是否读它取决于你没另设 `--out`」。
2. **【中低 · 无防腐烂】「本命令不跑 `extract()`」无任何机制守护**（§2.2）。
   preflight 零测试覆盖；将来加一步抽取会让文案静默变假话而 574 条测试全绿。
   建议：加一条针对 preflight 的结构测试（如断言其依赖里不出现 `extract`/`project`）。
3. **【低 · 可追溯性】`min_columns: 900` 的出处标注失真**（§3.2）。
   实际等于 `drop_rate.rs:65` 的 `MIN_PROPS`，但注释只说「按真实产物标定」，未提对齐关系，
   存在静默漂移风险。
4. **【低 · 欠限定】「实测过的假信心」未点名因果**（§3.3）。
   96.9% 只在「改成的名字查不到 CN 映射」时成立；同一构造换个名字就是 8.0% 绿。
   现文案易被读成「重名就会红」—— 恰是本需求要消灭的误解。
   建议改为点名「改名导致实体在投影里被整体丢弃」。
5. **【低 · 建议】warn 文案可补真正的判别量**（§1.3）。
6. **【低 · 测试债】`sanitize()` 无测试、无可触发路径**（§4.2）。
   逻辑走查正确，但「口令绝不回显」当前实际依赖 sqlx 不回显，而非打码本身。

## 7. 我自己引入、又清掉的问题（如实登记）

- **`out/extract-stats.json` 副作用**：`ontok-propose --project extracted` 会额外在
  工作目录下写 `out/extract-stats.json`。已确认它被 `.gitignore:5` 的 `out/` 覆盖，
  不污染版本控制；收尾已删除。
- **临时合成产物与日志**：全部落在 `.review-workspace/verify-req049/`，
  收尾已删（仅保留本报告）。**未在 `tests/` 下放过任何临时 Rust 测试**，
  因此 `git diff --stat HEAD`（tracked 变更）为空。
- **未做的事（自律）**：未 `ln -s`、未设 `CARGO_TARGET_DIR`、未 `--release`、未建 probe crate、
  未并发跑全量、未 `echo` 连接串、未 push（仓库无 remote）、未改历史、
  未对共享本体库做任何写/DELETE/TRUNCATE。
- **口径纪律**：所有数字均标注了口径与命令；求和用 `awk` 而非 `paste -sd+`；
  编译失败未计作变异执行（本轮无存活变异实验）。

## 8. 收尾状态

- 分支 `verify/REQ-049`，**HEAD = `7343bb0`**，已从基线对照的 detached HEAD 还原。
- `git diff HEAD --stat` **为空** ⇒ 无任何 tracked 文件被修改。
- `git status -uall` 仅余本报告所在的 `.review-workspace/verify-req049/`（未跟踪、仓库外产物，
  按任务书指定路径存放）。
