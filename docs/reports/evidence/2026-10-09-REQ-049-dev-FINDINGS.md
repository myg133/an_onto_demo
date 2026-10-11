# REQ-049 Dev 过程证据（边做边落盘）

工作区：`/home/myg133/Projects/Anchnet/Ontology/feature-REQ-049`
分支：`feature/REQ-049`　基线：`9d6c529`

---

## 1. 复现 QA 的 D1（假信心）—— 第一手实测，不引用二手结论

**人造产物口径**：读真产物 `fix-props/schema.json`（91 表 / 1051 列），
把 91 张表的 `name` 全部改成 `"same_table"`，**列内容一行不动**，
落 `req049/same-name-91.json`。

| 命令 | 结果 |
| --- | --- |
| `cargo run --bin ontok-gate-preflight -- --verify-only` | **EXIT=0**，输出「两个门禁现在都能读到它了」 |
| `cargo test --test drop_rate`（同 `ONTOK_SCHEMA_JSON`） | **EXIT=101**，丢弃率 **96.9%**（934/964 被丢弃） |

⚠️ **与 QA 报告的差异（如实记）**：QA 写的是 **100.0%**，我实测 **96.9%**。
口径差异未知（我的构造保留全部列；QA 的构造未说明）。**定性结论一致**
（preflight 绿 / 门禁红），但**数字不一致，不声称与 QA 相同**。

---

## 2. 关键代码事实（支撑 AC-001 的措辞，不是推测）

`src/bin/ontok-gate-preflight.rs` 只 `use ontok::extract::introspect::{introspect, SchemaReport, ...}`。
`grep extract|propose|table_names` 在该文件里**只命中这一行 import**。

⇒ 本命令**从不**调用 `extract()`、**从不**加载 `src/extract/table_names.json`、
**从不**调用 `project()`。而两个门禁三样都要（`tests/drop_rate.rs:95`、
`tests/test_fk_naming.rs:322`）。

⇒ **结构上就不可能**知道门禁会不会红。这不是谦虚，是可查的事实。

---

## 3. AC-002 为什么只是启发 —— 有反例，不是只有「没验证」

只让 **2 张**表同名（91 张里把 `activity_recommendations` 改名成 `activity`），
`req049/dup-two.json`：

| 命令 | 结果 |
| --- | --- |
| preflight | EXIT=0 + 警告「activity ×2」 |
| `cargo test --test drop_rate` | **EXIT=0，PASSED** |

⇒ **「重复表名 ⇒ 门禁会红」是假的**：部分重复时门禁**照样绿**。
⇒ 所以 `bad`（判失败）会是**误伤**，warn-only 是被实测支持的选择。
⇒ 报告里**不会**写「重复表名会导致门禁失败」。

依据（为什么重复值得看一眼）：`src/extract/introspect.rs` 折叠行时用
`tables.iter_mut().find(|t| t.name == tname)`，**同一勘查器不可能吐两个同名表**。
⇒ 重复表名说明产物**不是这条路径产出的**。这是**形状/来源**信号，不是语义判断。

真产物实测：91 张表 / **91 个不同名字 / 0 重复** ⇒ 真实产物上该警告**不会触发**。

---

## 4. AC-003 四种情形实测（全部改动后）

| # | 情形 | 命令 | 退出码 | 关键输出 |
| --- | --- | --- | --- | --- |
| 1 | 正常（真产物） | `preflight --verify-only` | **0** | 「两个门禁都能**读到并解析**这份文件」 |
| 2 | 源库不可达 | `preflight`（`ONTOK_SOURCE_DATABASE_URL` 指向 `127.0.0.1:1`） | **1** | 「源库不可达 / 勘查失败…」+ 完整三选一指引 |
| 3 | 产物非合法 JSON | `ONTOK_SCHEMA_JSON=req049/bad-json.json preflight --verify-only` | **1** | 「产物不是合法的 schema JSON（门禁会在这里直接 panic）」 |
| 4 | 表数异常偏少 | `ONTOK_SCHEMA_JSON=req049/few-tables.json preflight --verify-only` | **1** | 「表数异常偏少：50 张 < 下界 80」 |

情形 3 与 4 **退出码相同（1）但文案不同**，仍各自可区分。
AC-002 的表名唯一性落在 `warn`，**未新增任何失败路径**，故上表判别力零退化。

### 情形 2 的一个诚实限定

口令确实**没有**出现在输出里（用哨兵串 `SENTINEL_PW_NEVER_ECHO_9f3a` grep 验证，PASS）。
但本次 sqlx 报的错是 `pool timed out while waiting for an open connection`，
**错误文本里本来就没有连接串**，所以 `sanitize()` 的 `<userinfo-redacted>`
分支**没被这次运行触发**。
⇒ 本次证明了「该情形下不回显」，**没有**证明打码机制本身有效。
（打码逻辑是 REQ-048 既有代码，本需求未改动，也未为此新增测试 —— 会改动测试条数。）

---

## 5. 环境事实（复用，别再栽）

```bash
export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' \
  /home/myg133/Projects/Anchnet/Ontology/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"
```
`.env` 在仓库根；worktree 内没有。库名 `notol` → 必须替换成 `ontol`。
`relative URL without a base` = 取值残缺，不是环境不可达。

冷编译实测：~52s（本机），非 6 分钟。

---

## 6. 全量测试（`cargo test --no-fail-fast`，口径见下）

**口径**：把输出里每一条 `test result:` 的 `passed; / failed; / ignored;` 字段
逐行 awk 求和（**未**用 `paste -sd+`）。

```bash
grep "^test result:" full-test-2.txt | awk '{for(i=1;i<=NF;i++){
  if($i=="passed;")p+=$(i-1); if($i=="failed;")f+=$(i-1); if($i=="ignored;")g+=$(i-1);}
  END{printf "passed=%d failed=%d ignored=%d\n",p,f,g}'
```

| 轮次 | 结果 | 说明 |
| --- | --- | --- |
| 第 1 轮 | passed=573 **failed=1** ignored=1 | **环境类失败**，见下 |
| 第 2 轮 | **passed=574 failed=0 ignored=1**，退出码 0 | ✅ 与基线 `9d6c529` 完全一致 |

### 第 1 轮那次失败是**环境类**，不是代码失败（分开记）

- 失败项：`test_round_two_still_runs_when_latest_version_is_not_frozen`
- 所在二进制：`tests/test_extract_cli_req020.rs`（本需求**未改动**该文件）
- 报错：`连不上数据库：error communicating with database: Connection reset by peer (os error 104)`
- 复跑证据：单独跑该二进制 → **5 passed / 0 failed，EXIT=0**
- ⇒ 判定：**库连接被重置**的环境抖动。**复现率随负载浮动，不声称稳定值。**
- ⇒ 本需求只改 `src/bin/ontok-gate-preflight.rs` 一个文件，
  `tests/` 目录 diff 为空，红线三文件 diff 为空 —— **无法影响该测试**。

## 7. 产物与红线最终核对

- 产物 `fix-props/schema.json` sha 全程未变：
  `b71c2afde6c103132e4e1ca6b9896aab442bd80abbe1fcc0870f35ddce0f2019`
  （含跑完 4 个失败情形之后仍一致 ⇒ 现拉失败时确实没有覆盖产物）
- 改动文件：**仅** `src/bin/ontok-gate-preflight.rs`（+154 / −12）
- `tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs`：diff 为空
- `tests/` 全目录：diff 为空 ⇒ 测试条数零变化
- 未新增 / 未删除 / 未 `#[ignore]` 任何测试
- 仓库无 remote，**未 push**