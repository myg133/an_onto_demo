# QA 验收报告 · REQ-041 建连接有限重试 + 行序自检加固

| 项 | 值 |
| --- | --- |
| 被验提交 | `4f03d50`（分支 `verify/REQ-041`） |
| 基线 | `9ff76e6`（develop） |
| worktree | `/home/myg133/Projects/Anchnet/Ontology/verify-REQ-041` |
| 验收人 | Verifier（独立复验，未参与实现） |
| 日期 | 2026-10-09 |
| **结论** | **PASS 带非阻断**（7 项非阻断发现，0 项阻断） |

---

## 0. 一句话结论

核心功能、两道闸、AC-003 的范围约束在**交付物上全部成立**，且我用 15 条真跑的变异
（含 Dev 没做的 4 条绕过方式）复现并扩展了 Dev 的结论。
**不阻断的三件事**：(1) 源码级 AC-003 判据有 3 个可绕过的洞（当前代码是对的，护栏不够硬）；
(2) AC-006 撤回的**结论**站得住、**理由**有一处实质性错误，会误导后续 follow-up 的方向；
(3) 覆盖缺口不是 Dev 报的 5 处而是 **6 处**，且我这一轮实测就红在其中一处（基线上）。

---

## 1. 两道闸真的都有效吗？（Q1）

### 1.1 次数闸 —— 复现成立
`M1`：`CONNECT_RETRIES: 2 -> 0`。
命令 `cargo test --test test_connect_retry`（二进制 `test_connect_retry`，10 条）
⇒ **9 passed / 1 failed**，杀于 `test_connect_retry_succeeds_after_transient_failures`。
与 Dev 自述完全一致。

### 1.2 墙钟闸 —— 复现成立，且**确实独立于次数闸**（关键）

`M3a`（`2 -> 1_000_000`，墙钟不变）⇒ **5 passed / 5 failed，34.13s**，
杀于 `test_connect_retry_never_hangs_under_persistent_failure`。与 Dev 一致。

`M3b`（`while` 改 `loop`，两道闸全删）⇒ 杀于**同一条**判据。
口径差异如实记录：Dev 记「12.00s 被判据 timeout 掐停」；我实测该判据在 12s 变红，
但**其余 3 条会永远自旋的测试不会结束**，整个进程需靠外层 `timeout 300` 杀
（rc=124，elapsed=300s）。判据有效，Dev 的「12.00s」是判据触发时刻而非整轮耗时。

**独立性我用运行期探针直接读 `retry_connect` 的返回值验证**（临时探针，已删除）：
闭包 = 睡 200ms 后返回 `Err`，打印 `attempts / budget_exhausted / 墙钟`。

| 模式 | CONNECT_RETRIES | CONNECT_RETRY_BUDGET | attempts | budget_exhausted | 墙钟 |
| --- | --- | --- | --- | --- | --- |
| 原样 | 2 | 35s | 3 | **false** | 1.507s |
| m5 | 2（**不变**） | 500ms | 1 | **true** | 0.201s |
| m4 | 1_000_000 | 3s | 4 | **true** | 2.908s |
| m4real | 1_000_000 | **35s（真值）** | **18** | **true** | **33.740s** |

- **次数闸单独生效**：原样配置 `attempts=3`、`budget_exhausted=false`，墙钟从未参与。
- **墙钟闸单独生效**：`RETRIES=1_000_000` 而预算仍是真值 35s 时，**第 18 次尝试即被掐停**，
  次数还剩约 999,983 次没用，`budget_exhausted=true`。
- briefing 两个补做问：**只调大次数、墙钟不变 ⇒ 总耗时仍被 35s 掐住（33.74s）**；
  **只调墙钟、次数不变 ⇒ attempts 掉到 1，`budget_exhausted=true`**。两向都成立。
- 错误信息能区分两种停法（实测原文）：
  - 次数用完 →「…2 次重试全部用完，仍然失败。」
  - 撞墙钟 →「…撞上 35s 的墙钟上界（次数还没用完：上限给了 1000000 次重试），仍然失败。」

### 1.3 参数取值 `CONNECT_RETRIES = 2` 的独立评估

**结论：2 可辩护、量级正确，但 Dev 的成本论证权重放错了地方。**

站得住：①「主导成本是 10s/次而非退避」正确（退避合计仅 0.9s = 2.9%）；
②正常路径零开销**已实测**（`..._does_not_retry_or_wait_on_first_success` 断言 <100ms 通过）。

论证有偏：
1. **8.8% 是「524 条全部耗尽重试」的幻 worst case。** 重试成本只在「该测试本来就要红」时发生；
   而一次环境抖动的真实代价是「人工重新分类 + 重跑一整轮（~6 分钟）」，不是那 10~30s。
   在这个成本结构下，**多一次重试（10s）换一个「把红变绿」是划算的**。
2. **退避相对失败时间尺度太小**（300/600ms vs 10s 失败窗口），所以**次数比退避更值钱**——
   这恰恰支持「次数可以再加一次」，而非 Dev 的「再加就是纯烧时间」。

**我建议的更优方案（按性价比，均未实施）**：
1. **缩短重试轮的 `acquire_timeout` 而不是减少次数**：首发 10s、后续每轮 3s ⇒
   最坏 `10+3+3+0.9 = 16.9s`（vs 现状 30.9s），**次数不变、墙钟近乎减半**。
   共享集群实测正常 p50 ≈ 155~161ms，3s 仍有 ~20 倍余量。**这一条收益最大。**
2. 退避加 **jitter**（如 `wait × (0.5 + rand)`）：一个二进制内默认 8 线程并行，
   现在 8 条连接会**同步**重试，存在惊群。
3. 次数 2 改 3（须与第 1 条配套，总墙钟才不涨）。

**顺带发现判据隐患（非阻断，见 §9 F3）**：`test_retry_budget_exceeds_the_realistic_worst_case`
（`tests/test_connect_retry.rs:163`）把 `per_attempt = 10s` **硬编码**了一份，
声称它等于 `src/db/pool.rs:74` 的 `acquire_timeout`，但那是 inline 字面量、Rust 侧读不到
⇒ **两份真相**。若有人把 `acquire_timeout` 调成 30s，本判据仍绿，而真实最坏已 90.9s 远大于 35s
⇒ 墙钟闸会实际截断最后一次尝试，判据毫无反应。

---

## 2. 范围真的只限建连接吗？（AC-003，Q2）

### 2.1 交付物本身：成立
`tests/common/mod.rs:80` 实为
`match retry_connect(|| pool::connect_for_tests(&url, max_connections)).await {`
—— 一个函数调用、零其他语句。**迁移、断言、业务读写全在这个 await 之外**。AC-003 在交付物上成立。

### 2.2 `M2` 复现：成立
把 `migrate(&p)` 塞进闭包 ⇒ **9 passed / 1 failed**，杀于 `test_retry_scope_is_connect_only`。

### 2.3 「运行期判据证明不了 AC-003」—— 我**同意**
单个测试里「建连接失败」与「连上之后失败」的可观测表现确实完全一样（都是一条 panic、
都没有可用连接），差别只在被重试闭包的大小，而闭包大小运行期看不见。
我也没能构造出纯运行期判据：唯一可行的近似是给闭包装计数器、断言调用次数 = 1，
但计数器要穿过 `test_pool_with` 传出来，那本身已是源码层改动。
⇒ **同意必须用源码断言**，也同意沿用本仓 `include_str!` 的既有做法。

### 2.4 源码级判据 `test_retry_scope_is_connect_only` 的洞 —— **有洞，且不止一个**

判据取窗逻辑是 `code.find("retry_connect(||")` 之后**定长 200 字符**的裸切片。
我在**未变异的 4f03d50 原文**上复刻该逻辑，窗口实际内容是：

```
' pool::connect_for_tests(&url, max_connections)).await { Ok(p) => p, Err(e) => panic!("{e}"), }
  } pub const CONNECT_RETRIES: usize = 2; pub const CONNECT_BACKOFF_BASE_MS: u64 = 300; ...'
```

**这 200 字符越过了 `test_pool_with` 的函数体末尾，吃进了后面三个 `const` 声明。**
即它看的不是「闭包」，而是一条定长裸切片。我实测的 4 条绕过（Dev 未做）：

| 变异 | 改法 | 结果 | 判定 |
| --- | --- | --- | --- |
| **H1** | 闭包改调 `connect_for_tests_plus_schema()`，该 helper **连上后顺手 `migrate`**，名字含 `connect_for_tests` 子串但不含任何禁用词 | **10P/0F 存活** | **判据有洞** |
| **H2** | 闭包里 `connect_for_tests` 之后插 170 字符填充，把 `migrate` 顶出 200 字符窗口 | **10P/0F 存活** | **判据有洞** |
| **H3** | 无害空格 `retry_connect( ||` | 9P/1F 红 | 误报（脆，非洞） |
| **H4** | 文件更早处放诱饵 `retry_connect(||...)`，真正闭包塞 `migrate` | **10P/0F 存活** | **判据有洞** |

**H1 不是「只是看不见」，行为面也被绕过**（临时探针，已删）：把「先连上、再做一件
连上之后的事、那件事失败才 `Err`」包成 `Result<PgPool, DbError>` 形状交给 `retry_connect`，
闭包被调用 **3 次** ⇒ **连上之后的失败确实会被重试**。

同时，**Dev 宣称的「类型层 + 源码层双保险」不成立**：类型层挡不住 H1
（helper 返回类型就是 `Result<PgPool, DbError>`，完全兼容）。

**缓解事实（决定这是非阻断的原因）**：绕过 H1/H2/H4 都需要**有人主动去改代码**，
而当前代码是对的。护栏不够硬 ≠ 现在有缺陷。

**建议的加固方向（不属本次 scope，未实施）**：把禁用词黑名单 + 定长窗口
换成**配平括号提取闭包全文**（或直接断言 `retry_connect` 的实参**恒等于**
`pool::connect_for_tests(&url, max_connections)`），并把取「首个出现」改成
取 `test_pool_with` 内的那个。

---

## 3. 会不会掩盖真缺陷？（Q3）

### 3.1 「连上之后失败」不被重试 —— 两条独立实证
- **我自己造的**（E1）：把 `tests/test_db.rs:658` 的 `max_connections` 断言 `1` 改 `2`
  （该断言在 `test_pool().await` **之后**）。
  ⇒ `0 passed/1 failed`，`panicked at tests/test_db.rs:658:5: left: 1, right: 2`，
  **测试用时 0.30s**。若被重试掩盖应是 ~31s。对照组同一条 2.50s 绿。
- **Dev 第 1 轮日志里同一个二进制、同一轮两种形态并存**（最强证据）：
  - 建连接阶段失败 `test_snapshot_three_states_survive` →
    `panicked at tests/common/mod.rs:82:19` +「已尝试 **3** 次 … 2 次重试全部用完」= **被重试**
  - 连上之后失败 `test_snapshot_roundtrip` / `test_snapshot_resave_replaces_not_appends` →
    `panicked at tests/test_snapshot.rs:22:40`，落在 `ps.migrate().await.unwrap()`
    = **原样抛出、零重试**

⇒ Dev 说的「第 1 轮那 3 条 `Db(PoolTimedOut)` 发生在连上之后、照旧红」**核实成立**。

### 3.2 关键问题：连续多条语句 / 用着用着被 reset —— **不覆盖**
`retry_connect` **只包建池那一刻**。池建立之后、测试执行中途的连接失效
**一律不重试**，原样抛出（正是 3.1 里 `tests/test_snapshot.rs:22` 那两条的性质）。

所以对「一个测试里连续多条语句」的连接失效：**不会**被重试掩盖（因为根本没重试），
但也**不会被修复**。REQ-041 治的是「建池前握手抖动」，
**不治「池建好后连接被 reset」**。这一点 Dev 的报告没有明说，我在此明确。

---

## 4. AC-006 的撤回对不对？（Q4）

**裁定：撤回「结论」对，撤回「理由」错。**

二进制 `test_snapshot_write_paths`（8 条），每条真跑：

| # | 变体 | 做什么 | 结果 | 判读 |
| --- | --- | --- | --- | --- |
| M13c | 置换写在 **bind 处**：`push_bind(rows[(n+2)%rows.len()])`，`pushed` 照记原序 | **8P/0F**（15.85s） | **存活**（复现 Dev） |
| ctl | 同一置换，跑在**真正未加固**的 b5b368b 版本 | **8P/0F**（23.48s） | **存活** |
| X | 廉价加固（`kv` 单一真源）+ **恒等断言** `assert_eq!(pushed, keys)`，置换写在 `kv` 上 | **7P/1F**，红于 `:707` | **被杀** |
| Y | 同 X 加固，但置换**绕开 `kv`** 直接写在 bind 处 | **8P/0F**（13.87s） | **存活** |

1. **AC-006 规定的那个 3 行片段本身就是 no-op。**
   基线原文（`tests/test_snapshot_write_paths.rs:690-697`）已经是
   `pushed.push(*k)` 与 `.push_bind(*k)` 用**同一个** `*k`；提成 `let kv = ...` 再共用，
   行为逐字节不变、判别力增加为 0。
   ⇒「照 AC 字面做杀不掉 M13」= 事实，Dev 在 AC 字面范围内撤回站得住
   （AC-006 本身也留了「做不通就如实撤回」的口子）。

2. **Dev 的根因分析有一处实质性错误**（这条会影响 follow-up 的方向）：
   Dev 写「要拿到真正发出去的那串，就得让 key 走 **SQL 字面量**并从拼好的 SQL 里
   **解析回来** —— 那已经不是廉价加固」。
   **实测不成立**：变体 X **只加了一条恒等断言** `assert_eq!(pushed, keys, ...)`，
   **没改任何 SQL 构造方式、没做任何解析**，M13c 就被杀（红在 `:707`）。
   ⇒ 真正缺的不是「解析能力」，而是**把「记录器那份」与「期望」做恒等比较**这一步 —— 一行。
   若照 Dev 的说法开 follow-up，后一个 Dev 会去做「改 SQL 构造 + 写 SQL 解析器」，
   那是更大且不必要的改动。**建议在 follow-up 描述里更正这一条。**

3. **但 Dev 的更大直觉成立**：「没有哪一行能封死这一类」。
   变体 Y 证明：即便把加固做全（单一真源 + 恒等断言），置换只要绕开 `kv`
   直接写在 bind 处，recorder 记的仍是原序、恒等断言照样通过。
   ⇒ **recorder 类断言原理上抓不到 bind 侧置换**，这正是 REQ-040 QA 结论的实测复现。

### 关于 Dev 引用的「对照组」—— 该证据无效
- `tests/test_snapshot_write_paths.rs` 在 `9ff76e6` 与 `4f03d50` **逐字节相同**
  （`git show` 加 `sha256sum` 均为 `cdfe172127d5f321`）⇒ AC-006 加固**从未提交**。
- `m13c.log` 与 `m13c_control.log` 的**测试二进制名与哈希完全相同**
  （`test_snapshot_write_paths-4119f8d8fad5cac3`），两次编译都 1.01s
  ⇒ **对照组编译的就是变异组自己**，是同义反复，**不能**证明「加固零判别力」。
- 真正的对照组是 `b5b368b`（sha `ef831b3de8aaf07e`：断言直接写在 `insert_legacy_actions` 里、
  只断言入参 `keys`、无 `pushed` 记录器）。**我自己补跑了这一条（变体 ctl）**：
  M13c 在真正未加固的基线上**同样存活**。
  ⇒ Dev 想说的那句话**是真的**，只是他引用的证据不成立。**结论不受影响。**

---

## 5. 重试覆盖缺口（Q5）

### 5.1 核实：5 处都存在且都不走 `test_pool()` —— 成立
| 位置 | 形态（裸 grep 核实） |
| --- | --- |
| `tests/test_db.rs:142` | `ontok::db::pool::connect(&url).await.expect("连不上测试库")` |
| `tests/test_db.rs:164` | 自建 `PgPoolOptions` + `after_connect`（FANOUT 预热池）`.connect(&url)` |
| `tests/test_db.rs:321/323` | 自建 `after_connect`（`search_path = pg_temp`）`.connect(url)` |
| `tests/test_db.rs:606/608` | 自建 `after_connect` `.connect(url)` |
| `src/db/apply.rs:219` | `crate::db::pool::connect_for_tests(&url, 1)`（生产代码里的 `#[cfg(test)]`） |

**两点修正 Dev 的描述**：
1. **`:142` 的理由不成立。** 它是 `pool::connect(&url)`，**没有** `after_connect` 钩子，
   返回类型就是 `Result<PgPool, DbError>`，与 `retry_connect` 签名**完全兼容**
   ⇒ 结构上**可以**直接套 `retry_connect(|| pool::connect(&url))`。
   Dev 说「前 4 处结构上确实不能改成 `test_pool()`」——就 `142` 而言，
   它确实不能改 `test_pool()`（要 `max_connections=5` 的管理员连接），
   但**能**套重试，这两件事被混为一谈。
2. **实际是 6 处，不是 5 处。** 见 5.3。

### 5.2 建议：**记 follow-up，不在本需求补齐**
理由：
- AC-001 把范围明确划在 `test_pool()`；覆盖 `:164/:323/:608` 需要把 `retry_connect`
  泛化成「包住任意 `Future<Result<PgPool, _>>`」并支持自定义 options，是一次真重构。
- `src/db/apply.rs:219` 在 `src/`（生产代码的 `#[cfg(test)]`），
  为测试连通性去动 `src/` 是 scope 问题。
- 5.3 那第 6 处**根本不是 `retry_connect` 能解决的**。

但 follow-up 的描述必须改三处：① 处数 5 改 **6**；② `:142` **可**套重试，成本最低、收益直接；
③ 第 6 处需要**另一类**药（见下）。

### 5.3 第 6 处：CLI 子进程路径（本轮实测真的红了）
`tests/test_extract_cli_req019.rs` 的 `run_cli()` 会**拉起 `ontok` 二进制子进程**，
子进程走**生产路径** `pool::connect`（`src/db/pool.rs:26`），**完全不经过 `retry_connect`**。

**我这一轮基线对照（`9ff76e6`，未加固）实测红的就是这一条**：
```
test_round_two_loads_existing_types_and_detects_display_name_change ... FAILED
panicked at tests/test_extract_cli_req019.rs:159:5:
第 2 轮应当跑通，stderr：Error: 数据库错误：error communicating with database:
Connection reset by peer (os error 104)
```
⇒ 这是**环境类**失败（连接被 reset，非断言失败），且发生在**基线**上，
**因此在归因上不可能由 REQ-041 引起**；反过来也证明**这处缺口是活的**。
要治它得决定「生产连接要不要重试」，那是独立需求。

### 5.4 「REQ-041 减少但不消除环境类红灯」—— 独立评估：**成立，且被低估了**
- 「减少」：成立。`test_pool()` 覆盖了绝大多数测试，握手抖动被吸收。
- 「不消除」：成立，本轮基线那条 CLI 子进程失败就是活证据。
- **但要补一句 Dev 没说的**：REQ-041 **完全不覆盖「池建好后连接被 reset」**（§3.2），
  而高负载时段 `os error 104` 恰恰大量以这种形态出现（Dev 第 1 轮那条
  `test_migrate_on_empty_schema_survives_concurrent_first_run` 就在 `tests/test_db.rs:164`，
  属「一撞就炸」那 4 处之一）。
  ⇒ 覆盖面比「只差 5 处」的描述更窄。

---

## 6. 全量、红线、产物（Q6）

### 6.1 两轮全量（同会话、同环境、8 核、热 target、库名 `ontol`）
命令：`cargo test --no-fail-fast`

| 轮次 | 代码 | passed | failed | ignored | 总墙钟 | 其中编译 | 测试净时长 | 测试目标数 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 对照组 | `9ff76e6` | **523** | **1** | 1 | **342s (5m42s)** | 6.65s | ~335.4s | 35 |
| 本次 | `4f03d50` | **534** | **0** | 1 | **376s (6m16s)** | 17.48s | ~358.5s | 36 |

口径说明（避免歧义）：
- 计数口径 = 日志里全部 `test result:` 行按 `passed/failed/ignored` 求和。
- 「测试目标数」= `test result:` 行数；拆解为
  `4f03d50` = **31** 个集成测试二进制（30 加新增 `test_connect_retry`）+ 4 个 unittest + 1 个 doc-test；
  `9ff76e6` = 30 + 4 + 1。Dev 记作「36 个二进制」，精确说是 36 个**测试目标**。
- passed 差 `534 - 523 = 11` = 新增 10 条判据 + 基线上失败的那 1 条本次通过。**账目自洽。**

**AC-005 判定：达标。**
- 期望值：AC-005 原文写「524 passed」，那是**加判据之前**的数。
  正确期望是 `524 + 10 = 534`，Dev 报的 534 正确。**AC-005 的字面数字该更新（文档小瑕）。**
- 耗时：`+34s`（含编译）/ `+23s`（扣编译）= **+6.9%**。
  **但我不声称这是「重试的代价」**：本轮 `4f03d50` 零失败 ⇒ 重试一次都没被触发
  （正常路径零开销已实测），这 +23s 只能是负载/顺序噪声。
  对照 Dev 自己的数据：其对照组 6m26.1s vs 第 3 轮 5m55.1s，**改动后反而快 31s**。
  ⇒ **两轮样本都不足以分辨 ±30s 的差异；诚实说法是「无可测量的劣化」，
  而不是「精确测得 +6.9%」。** 这与 AC-002「显著小于一轮全量」不冲突。

**环境类 vs 代码失败（严格二分）**
| 类别 | 条数 | 明细 |
| --- | --- | --- |
| **代码失败** | **0** | 两轮均无断言失败、无编译错误 |
| **环境类** | **1**（仅对照组） | `test_round_two_...display_name_change`，`Connection reset by peer (os error 104)`，发生在 CLI 子进程内，**未加固基线上**，与本改动无因果关系 |

**集群状态**：本轮集群整体可用（两轮共 35+36 个目标，仅 1 条环境类失败）。
我未测 `SELECT 1` p50（本机无 `psql`/psycopg，且纪律禁止另建 probe crate），
故**不声称延迟基线**。**复现率随负载浮动，不给稳定值。**

### 6.2 红线核查 —— 全部通过
| 红线 | 核查方式与结果 |
| --- | --- |
| `tests/drop_rate.rs` 禁改 | `git diff --name-status 9ff76e6 4f03d50` 仅 `M tests/common/mod.rs` 加 `A tests/test_connect_retry.rs` ⇒ **零改动** |
| `tests/test_fk_naming.rs` 禁改 | 同上 ⇒ **零改动** |
| `src/domain/view.rs` 禁改 | 同上 ⇒ **零改动**（构造上即满足） |
| `test_humanize_still_refuses_unknown_columns` 断言不得放宽 | 4 个 bad 列仍逐个 `assert_eq!(humanize_column(bad), None, ...)` ⇒ **一条未动** |
| 任何测试不得删除 / `#[ignore]` | 全 diff 仅 **3 个删除行**，全是 `common/mod.rs` 原来的 `.expect("连不上测试库")` 三行；`#[ignore]` 计数 基线 **2** 等于 改动后 **2**（均在 `tests/test_mcp_server.rs`，其一为注释）⇒ **未删测试、未加 ignore** |
| `test_backfill_...` 不得放松断言 | `tests/test_snapshot_write_paths.rs` 对基线 diff = **0 行**，39 条 `assert` 完好 ⇒ **未放松** |
| `.review-workspace/fix-props/schema.json` 绝不可动 | 存在，219688 字节，mtime `10月 7日 23:55`（早于本会话）⇒ **未碰** |
| 无 remote / 不 push / 不改历史 | 全程只做 `git checkout`（切分支与还原），**未 push、未 squash、未改历史**；结束态已还原 |

### 6.3 产物 sha —— 逐字节复现成功
命令（worktree 根；round 1 不连库；`--out` 在 `--stats` 之前）：
```
cargo run --bin ontok-propose -- \
  --schema /home/.../.review-workspace/fix-props/schema.json \
  --names src/extract/table_names.json --project extracted \
  --out <scratch>/proposal.json --stats <scratch>/stats.json
```
| 产物 | 实测 sha256 | 预期 | 结论 |
| --- | --- | --- | --- |
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | 同 | 一致 |
| `proposal.json` | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | 同 | 一致 |

---

## 7. 变异测试总表

全部在 `verify-REQ-041` 热 target 上真跑。**编译失败一律不算变异执行**；
「杀死」只指**代码判据**变红，**绝不用 `PoolTimedOut`/`ConnectionReset` 冒充杀死**
（`test_connect_retry` 全程不连库，结构上不可能混入环境类失败）。

| 变异 | 改法 | 二进制 | 运行条数 | 结果 | 杀死它的判据 |
| --- | --- | --- | --- | --- | --- |
| M1 | `CONNECT_RETRIES 2` 改 `0` | `test_connect_retry` | 10（9P/1F） | **杀死** | `test_connect_retry_succeeds_after_transient_failures` |
| M2 | `migrate(&p)` 塞进闭包 | `test_connect_retry` | 10（9P/1F） | **杀死** | `test_retry_scope_is_connect_only` |
| M3a | `2` 改 `1_000_000` | `test_connect_retry` | 10（5P/5F, 34.13s） | **杀死** | `test_connect_retry_never_hangs_under_persistent_failure` |
| M3b | `while` 改 `loop`，两闸全删 | `test_connect_retry` | 判据红 + 进程外层 300s 掐停 | **杀死** | `test_connect_retry_never_hangs_under_persistent_failure` |
| M4 | `2` 改 `1e6` **且** 预算 `35s` 改 `3s` | `test_connect_retry` | 10（6P/4F） | **杀死** | 预算合理性 + 可读性 + 次数三判据 |
| M5 | **只**改预算 `35s` 改 `500ms` | `test_connect_retry` | 10（5P/5F） | **杀死** | `test_retry_budget_exceeds_the_realistic_worst_case` 等 |
| H1 | 闭包调「连上后顺手 migrate」的 helper（名字含 `connect_for_tests` 子串） | `test_connect_retry` | 10（**10P/0F**） | **存活** | —— 判据有洞 |
| H2 | 填充 170 字符把 `migrate` 顶出 200 字符窗口 | `test_connect_retry` | 10（**10P/0F**） | **存活** | —— 判据有洞 |
| H3 | 无害空格 `retry_connect( ||` | `test_connect_retry` | 10（9P/1F） | **杀死** | `test_retry_scope_is_connect_only`（**误报**） |
| H4 | 更早处放诱饵 `retry_connect(||...)` | `test_connect_retry` | 10（**10P/0F**） | **存活** | —— 判据有洞 |
| M13c | 置换写在 bind 处，recorder 记原序 | `test_snapshot_write_paths` | 8（**8P/0F**） | **存活** | —— 与 Dev 一致（性质断言抓不到 bind 侧） |
| ctl | 同上，跑在真正未加固的 `b5b368b` | `test_snapshot_write_paths` | 8（**8P/0F**） | **存活** | —— REQ-040 加固对 M13c 零判别力（**真对照组**） |
| X | 单一真源 `kv` + 恒等断言，置换写在 `kv` | `test_snapshot_write_paths` | 8（7P/1F） | **杀死** | `assert_eq!(pushed, keys)` @ `:707` |
| Y | 同 X 加固，置换绕开 `kv` 写在 bind 处 | `test_snapshot_write_paths` | 8（**8P/0F**） | **存活** | —— recorder 类断言原理上抓不到 |
| Q3 | `test_db.rs:658` 断言 `1` 改 `2`（连上之后） | `test_db` | 11（1 跑） | **杀死**（0.30s 暴露） | 该断言本身；**未触发任何重试** |

**存活变异逐条论证**
- **H1 / H2 / H4 = 判据有洞（不是等价变异）。** 判据的承诺是「闭包里只有建连接」，
  而三条都能让闭包**真的**含上 migrate 却让判据全绿。行为面已用探针证明
  「连上之后的失败会被重试 3 次」⇒ 不是等价变异，是护栏失效。
- **M13c / ctl / Y = 判据有洞（与 REQ-040 QA 既有结论一致）。**
  断言的是**性质**（插入序 ≠ 字典序）而非**恒等**；任何「非恒等且非字典序」的置换都保持该性质。
  Y 进一步证明：即便补上恒等断言，只要置换绕开单一真源写在 bind 处，仍抓不到。
- **M3b 的存活子项（非变异，记账）**：`test_connect_retry_gives_up_on_persistent_failure` 等
  3 条在真无界下**永不返回**，被判据 timeout 掐停后进程仍需外层杀。
  这本身说明**判据设计得当**（用 timeout 而不是让套件挂死），不是缺陷。

**无「编译失败冒充变异执行」**：Dev 第一版 M3 用 `usize::MAX` 编译失败已作废（其 NOTES 有登记），
我用 `1_000_000` 与真 `loop` 重跑，M1/M2 日志经 `grep -E 'could not compile|^error\[E'` 确认无编译错误
（其中的 `error` 是 cargo 的 `error: test failed` 收尾行）。

---

## 8. 对 Dev 三个请求拍板项的独立看法

| 拍板项 | 我的裁定 | 依据 |
| --- | --- | --- |
| **墙钟闸是否真独立于次数闸** | **真独立，双向都成立。** | 探针实测：`RETRIES=1e6` 加真值 35s ⇒ 第 **18** 次尝试被掐停（还剩约 999,983 次），`budget_exhausted=true`；原样配置 `attempts=3`、`budget_exhausted=false`。两个补做问也都是「是」。 |
| **覆盖缺口该补还是记 follow-up** | **记 follow-up**，但 follow-up 描述要改：处数 **5 改 6**、`:142` 其实**可以**套重试（成本最低）、第 6 处（CLI 子进程）需要另一类药。 | §5。`test_db.rs:142` 无 `after_connect`、签名与 `retry_connect` 完全兼容；第 6 处走生产 `pool::connect`，`retry_connect` 够不着，且我这一轮基线就红在它上面。 |
| **AC-006 撤回对不对** | **结论对、理由错。** 撤回在 AC 字面范围内站得住（规定的 3 行片段是 no-op）；但「必须改 SQL 字面量 + 解析」被实测推翻（一条恒等断言即可杀死 M13c）。**建议更正 follow-up 描述**，否则下一个 Dev 会去做不必要的大改。 | §4。变体 X：只加 `assert_eq!(pushed, keys)`，红在 `:707`；变体 Y 证明 Dev「没有哪一行能封死这一类」的大直觉仍成立。 |

---

## 9. 发现但未修的问题（均非阻断，未擅自改交付物）

| # | 严重度 | 问题 | 位置 | 触发条件 | 影响 |
| --- | --- | --- | --- | --- | --- |
| **F1** | 中 | AC-003 源码判据取「定长 200 字符裸切片」加禁用词黑名单，窗口实际越过函数体末尾吃进后面 3 个 `const` | `tests/test_connect_retry.rs:244-266` | 有人把 post-connect 操作塞进闭包，用不含禁用词的名字（H1）、或闭包多写几行（H2）、或文件更早处出现第二个 `retry_connect(||`（H4） | **护栏失效**：真 bug 范围写宽可全绿通过。当前代码正确，故非阻断 |
| **F2** | 低 | `ConnectRetryError` 的 Display 用 `.as_secs()` 打印预算，`BUDGET` 小于 1s 时输出「撞上 **0s** 的墙钟上界」 | `tests/common/mod.rs`（Display impl） | 有人把预算调到 1s 以下 | 错误信息误导。当前 35s 不触发 |
| **F3** | 低 | 预算合理性判据硬编码 `per_attempt = 10s`，与 `src/db/pool.rs:74` 的 inline `acquire_timeout` 形成两份真相 | `tests/test_connect_retry.rs:163` vs `src/db/pool.rs:74` | 有人调大 `acquire_timeout` | 判据仍绿而墙钟闸实际截断最后一次尝试，「35s 不是拍脑袋」的论证失效 |
| **F4** | 低 | 退避无 jitter，一个二进制内 8 线程会同步重试 | `tests/common/mod.rs:214-236` | 并发测试 + 集群抖动 | 惊群；建议加 `wait × (0.5+rand)` |
| **F5** | 低 | 覆盖缺口实为 6 处（Dev 报 5），且不覆盖「池建好后连接被 reset」 | 见 §5 | 集群高负载 | 环境类红灯仍会发生；本轮基线实测红在第 6 处 |
| **F6** | 文档 | AC-005 写的期望值「524 passed」是加判据前的数，正确应为 534 | `BA/demands/REQ-041/acceptance.md:75` | — | 判读时可能误判为「多出 10 条」 |
| **F7** | 证据 | Dev 的 AC-006「对照组」与变异组编译的是**同一份代码**（二进制哈希相同），该证据无效 | `.review-workspace/req041/m13c_control.log` | — | 结论本身不受影响（我补跑了真对照组 ctl） |

---

## 10. 我自己引入又清掉的问题（如实报）

1. **环境变量库名搞错，误判成集群故障。**
   我最初直接 source `/home/.../Ontology/.env`，该文件库名是 `notol`，
   而 AC-000 环境事实要求 `notol` 换 **`ontol`**。后果：所有 DB 测试在
   `tests/common/mod.rs:82` 报 `database "notol" does not exist`，
   我一度把它读成「集群高负载、连接被拒」，并据此写过一句错误的环境判断。
   **修正**：新增 `$SC/envload.sh`，source 后做库名替换再 export；
   脚本内**不含口令**、**不打印连接串**（只打印布尔校验 `ENV OK`）。
   **Q3 与 AC-006 的全部实验已在修正后重跑**，本报告数字均为修正后口径。
   未变异二进制不受影响（`test_connect_retry` 全程不连库），M1~M5 / H1~H4 结论不变。

2. **进程纪律偏离（已按补救条款执行并登记）。**
   briefing 要求「写含反引号的 Markdown 必须用 edit/write 工具」。
   **本会话的运行时未提供 `write`/`edit` 工具**（仅有 read/bash/grep/glob/web_fetch/task_*），
   探测调用返回 `Write tool not found`。
   ⇒ 按 briefing 的兜底条款「失败就用带引号 heredoc 兜底并登记偏离」，
   本报告及全部中间笔记均用**带引号 heredoc**（`<<'ZZEOF'`）写入，
   每次只写一次、未反复重试。**登记为偏离。**

3. **一次读到了被变异的源文件。** 在 M3b 运行期间读 `tests/common/mod.rs`，
   拿到的是变异版（308 行 vs 原 323 行）。发现后改为「变异运行期间只用
   `git show <commit>:<path>` 读提交内容、不读工作区」，未影响任何结论。

4. **所有变异与探针均已清除。** 结束态实测：`git status --short` 为空、
   `HEAD=4f03d50`、`branch=verify/REQ-041`、
   `target -> /home/.../code/target` 软链**保持原样未删**、
   还原后再跑 `cargo test --test test_connect_retry` = **10 passed**。
   临时文件只落在 `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/verify-req041/`
   （**未用 `/tmp`、未用 home 目录**）。全程**未 push、未改历史、未执行任何删除操作**。

---

## 11. VERDICT

**PASS 带非阻断**

- AC-001 通过 / AC-002 通过（两道闸独立且均有判别力）/ AC-003 通过（交付物成立；护栏有洞但非阻断）
- AC-004 通过（15 条真跑变异，存活变异全部逐条论证；无编译失败冒充）
- AC-005 通过（534/0/1，无可测量劣化；AC 字面数字该订正，见 F6）
- AC-006 **撤回成立**（结论对、理由错，见 §4，建议更正 follow-up 描述）
- 红线全部通过；产物 sha 逐字节一致

**建议的 follow-up（不阻断本需求合入）**
1. 把 AC-003 判据从「定长窗口 + 禁用词黑名单」改为**配平括号提取闭包全文**并断言实参**恒等于**建连接（F1）。
2. `test_db.rs:142` 套 `retry_connect`（成本最低）；`:164/:323/:608` 需先把重试泛化成可包任意
   `Future<Result<PgPool,_>>`（F5）。
3. 单独立项决定「生产 `pool::connect`（含 CLI 子进程）要不要重试」—— 本轮基线实测红在它上面（F5）。
4. 把 `acquire_timeout` 提成具名 const，令预算判据直接引用，消除两份真相（F3）。
5. 退避加 jitter（F4）。
6. **更正 AC-006 的 follow-up 描述**：杀死 M13c 只需一条恒等断言，**不需要**改 SQL 为字面量或写解析器。
