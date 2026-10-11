# REQ-040 快速修复轮 · QA 复验报告

- 被验分支 / HEAD：`verify/REQ-040` @ `9ff76e6`（Dev 修复轮提交）
- 上一轮：`b5b368b`
- 复验 worktree：`/home/myg133/Projects/Anchnet/Ontology/verify-REQ-040`（结束前已恢复干净，HEAD 仍为 `9ff76e6`）
- 前人现场：`.review-workspace/verify-req040b/`（12 条变异结果 + 构造脚本 + `env.sh`）
- 本次产物：`.review-workspace/verify-req040c/`
- 结论：**PASS 带非阻断**（详见 §7；核心论证成立，但「确定性判据彻底堵死」这一目标未达成，有 3 条 DB 取证的分离路径存活）

## 0. 口径声明（重要，先读）

本报告所有数字的口径：

- **一轮（round）** = `cargo test --test test_snapshot_write_paths` 的一次完整运行。
- 该测试二进制 `test_snapshot_write_paths-4119f8d8fad5cac3` **内含 8 个 `#[tokio::test]`**，不是 5 个。
  前人 `results.txt` 里的 `of=5` 是**重复轮数 N=5**，不是「5 条测试」；简报表格把它读成「5 条测试」是口径误读。
  前人 `results.txt` 首行 `BINARY=` 为**空**（其 grep 未匹配到二进制名），即前人从未记录二进制名；本报告补齐。
- 因此「杀死型」变异的真实单轮结果是 **8 条里红 1 条（7 passed; 1 failed）**，不是「0 passed / 5 failed」。
- 环境类失败（`Connect(PoolTimedOut)` / `Connection reset by peer`）**不计为杀死**，单独归类。

## 1. 前人结果可信吗 —— 是，但口径需更正

亲���复现（各自 N=2，`test_snapshot_write_paths-4119f8d8fad5cac3`）：

| 变异 | 前人记录 | 本人实测 | 是否一致 |
|---|---|---|---|
| M10 | 0/5「红在 :625」 | **killed 2/2**，每轮 `7 passed; 1 failed`，红点 `test_snapshot_write_paths.rs:625` | 结论一致，口径不同 |
| M11 | 0/5「红在 :625」 | **killed 2/2**，同上 | 结论一致，口径不同 |

`results.txt` 非过期、非伪造。唯一缺陷是口径与二进制名缺失（见 §0）。

前人 M12 / M10b / M2b / M5 / M9 / M14 六条**本人未复核**，沿用前人「0/5，红在 :625 / :756 / :683」的记录，标注为**未独立验证**。

## 2. M15b —— Dev 的核心论证**成立** ✅

构造（`apply.py:M15b`）：断言从 `execute` 之前挪到之后 + 真实切片反序。

实测：**killed 2/2**，每轮 `7 passed; 1 failed`，红点 `test_snapshot_write_paths.rs:625`，失败用例
`test_backfill_is_deterministic_preserves_and_idempotent`。

Dev 的说法「挪到 `execute` 之后仍红，只是先把行写进库」**完全正确**：
- 断言主体 `pushed` 与 `execute` 无交互，`execute` 不修改 `pushed`，因此搬位置不改变判定结果；
- 判定只看「记录序列 ≠ 其字典序」，反序切片让记录序列变成字典序 ⇒ 仍然红；
- 红的位置仍在 `assert_ne!`（`:625`），只是发生在 `execute` 之后 ⇒ 副作用是「行已写库才报错」。

补充一条 Dev 未明说但正确的推论：`push_values` 只拼 SQL 不发网络，所以断言放在 `execute` 之前时，
**红 = 一行都没写进库**；这是当前 `:702` 位置的真正价值所在。

## 3. M13 / M15 / M16 逐条论证（存活 = 等价 or 判据有洞？）

判定方法：给存活变异加一段**只读回读**（`SELECT key, ctid … ORDER BY ctid`），
直接读出**物理行序**，并与字典序比对。为证明回读可信，先跑**对照** `baselineprobe`（未变异代码 + 同一段回读）。

| 变异 | 单轮结果 | 物理行序回读 | 前提是否被破坏 | 定性 |
|---|---|---|---|---|
| `baselineprobe`（对照） | 8 passed | `["z_last","m_mid","a_first"]`，`is_dict_ordered=**false**` | 否（正确） | **回读可信** ✅ |
| **M13** | 8 passed（存活 2/2） | `["a_first","m_mid","z_last"]`，`is_dict_ordered=**true**` | **是** | **判据有洞（严重）** |
| M15 | 8 passed（存活 2/2） | 未破坏（仅搬位置，不改顺序） | 否 | **等价变异** |
| M16 | 8 passed（存活 2/2） | `["z_last","m_mid","a_first"]`，`is_dict_ordered=**false**` | 否 | **等价变异**（附弱化） |
| **M17**（本人新构造） | 8 passed（存活 2/2） | `["a_first","m_mid","z_last"]`，`is_dict_ordered=**true**` | **是** | **判据有洞（严重）** |
| **M19**（本人新构造） | 8 passed（存活 2/2） | `["a_first","m_mid","z_last"]`，`is_dict_ordered=**true**` | **是** | **判据有洞（严重）** |

- **M13**：把 `.push_bind(*k)` 换成对 `*k` 的置换。recorder 仍记原始 `*k`（非字典序 ⇒ 绿），
  而**真正写进 `key` 列的是置换后的值**，物理行序因而变成字典序。
  ⇒ 前提「插入顺序 ≠ key 字典序」在**库里被真实破坏**，测试却全绿。**不是等价变异，是判据失效且无声。**
- **M15**：只把 `assert(&pushed)` 挪到 `execute` 之后。`pushed` 内容不变，未变异顺序下恒绿 ⇒ 行为等价。
  它单独存活**不构成问题**，只有与「顺序破坏」组合（即 M15b）才有意义——而 M15b 已红。
- **M16**：recorder 记一个「撒谎但仍非字典序」的序列。DB 回读证明**物理行序未变**（仍 `is_dict_ordered=false`），
  前提完好、测试绿得**正确** ⇒ 等价变异。但它证明「recorder 不是可信证人」：单改 recorder 一行即可让断言检查幻影序列，
  属**潜在弱化**（当前不致错，与顺序破坏组合才会致错）。

## 4. 第四种分离路径 —— **存在，且存活**（本人独立构造）

前人已试到 M16。本人独立构造 2 条，均命中同一类：**让 recorder 记的东西与真实 push 的东西解耦**。

- **M17（解耦 recorder）**：`pushed` 改由**另一条独立的 `push_values`**（对 `rows` 的 dry-run）填充，
  真实 INSERT 喂**排序副本**。recorder 序列仍非字典序 ⇒ 绿；物理行序真变字典序 ⇒ **存活**。
- **M19（按下标伪造 recorder）**：recorder 记固定序列 `["m_mid","z_last","a_first"]`（非字典序），
  真实 INSERT 喂排序副本 ⇒ **存活**。

两条均经 DB 回读取证（`is_dict_ordered=true`）且对照 `baselineprobe` 证明回读可信。

**结论**：Dev 的绑定**不是不可分离的**。它把「换一份切片喂给 `push_values`」这条捷径堵死了（M10/M11/M12 已红），
但没有堵死「让 `pushed` 不再来自那条 `push_values`」这一类。**「确定性判据彻底失效」这一目标未达成。**

## 5. 可长期维护性（独立评估）

Dev 的主张：「断言钉在插入路径自身的产物上」能防止将来无意重构掉绑定。

**我的独立判断：这是「失败模式的量变」，不是「质变」，且方向正确 —— 值一个便宜的加固，但不构成保证。**

支持 Dev 的部分（成立）：
- 修复前 `pushed`（被断言）与喂给 `push_values` 的切片是两个互不相干的东西，中间插一句 `push_values(&sorted, …)`
  自检照样绿 —— 这是**顺手重构就会踩到**的低门槛陷阱。
- 修复后 `pushed` 在**做绑定的那同一个闭包、同一次迭代里**被填充。要分离，必须
  (a) 改 recorder 那一行、(b) 改 `push_bind` 那一行、或 (c) 另起一次 `push_values`/预遍历。
  三者**都要动手改自检机制本身**，门槛从「无意的形参重构」升到「有意改安全网」。
  ⇒ 把「容易无意中发生的静默失效」换成了「必须有意为之的静默失效」。这是真实收益。

不支持的部分（本人证据）：
- 「有意为之」并不等于「不会发生」。M13 / M17 / M19 三条全部是**有意改自检机制**，全部静默存活。
- 更精确地说：当前 recorder 绑的是**迭代变量 `k`**，而真正决定物理行序的是**被绑定进 `key` 列的那个值**。
  M13 正是从这道缝（`pushed.push(*k)` 与 `.push_bind(<f(k)>)` 之间）钻过去的。
  一处更紧的写法是把键先算一次、同一份值既 `pushed.push` 又 `push_bind`（见 §7 建议），
  这能堵住 M13 一类，但**堵不住 M17/M19**（它们在迭代层面解耦）。
- 还有一条**原理性**上限：物理行序是堆页里的物理量，进程内无法可靠断言
  （Dev 自己在 `tests/test_snapshot_write_paths.rs:619-621` 写了 `ctid` 文本序在 `offset ≥ 10` 会排错）。
  所以**任何进程内断言都不可能完备**；能做到的只是抬高作恶门槛。这一点应当写进设计文档，而不是留给后人猜。

## 6. 全量与红线

**全量**（`cargo test --no-fail-fast`，worktree 根，2026-10-09 09:52 起）：
- 退出码 `0`，耗时 **332 s**
- **35 个测试二进制，524 passed，0 failed，1 ignored**
- 唯一的 `#[ignore]` 是 `tests/test_mcp_server.rs:553` 的 `todo_identifier_defenses_have_own_assertions`，
  `develop` 上已存在，与 REQ-040 无关。

**红线核查**（全部用 `git show <ref>:<path>` 取提交对象，避开工作树，避免与在跑的变异互相污染）：

| 红线 | 结果 | 证据 |
|---|---|---|
| `src/` 零改动 | ✅ | `git diff --stat develop...9ff76e6 -- src/` 为空 |
| `tests/drop_rate.rs` 零改动 | ✅ | `git diff develop...9ff76e6` 无该文件 |
| `tests/test_fk_naming.rs` 零改动 | ✅ | 同上 |
| `src/domain/view.rs` 零改动 | ✅ | HEAD 与 develop 的 SHA256 **完全相同**（`c0c2df2c…56e6`） |
| `test_humanize_still_refuses_unknown_columns` 一条断言未放宽 | ✅ | 该测试位于 `src/domain/view.rs:1254`（`#[test]`），断言 `:1261-1265`；该文件与 develop 逐字节相同 |
| 无测试被删除 / 无新增 `#[ignore]` | ✅ | diff 中 `^-.*#\[tokio::test\]|async fn test_` 为空；`^\+.*#\[ignore\]` 为空 |
| 三条回填性质断言未改 | ✅ | `assert_eq!` 13→13、`assert!` 20→20 条数不变；三条消息原文见 `:790` `:800` `:812` `:817` |

**一处需要如实说明的位移（非放宽）**：`assert_eq!(…vec![0,0,0], "前提：迁移前历史行应当全是 DEFAULT 0")`
从 `v_written` 两次 INSERT **之后**挪到了 `v_legacy` INSERT **紧后**。`git diff` 显示这是**同一个** `assert_eq!` 的移动：
表达式、期望值 `vec![0,0,0]`、消息三者**逐字未改**，只是位置变了（Dev 在 `:747-757` 的注释里也写明了这一意图）。
⇒ 判为「消除竞态窗口的位置调整」，**不是断言放宽**。

**产物 sha**（命令：`cargo run --quiet --bin ontok-propose -- --schema … --names src/extract/table_names.json --project extracted --out <out> --stats <stats>`，`--out` 在 `--stats` 之前；round 默认 1 ⇒ 不连库）：

| 产物 | 实测 sha256 | 期望 | 结论 |
|---|---|---|---|
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | 同左 | ✅ 完全一致 |
| `proposal.json`（`--project extracted`） | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | 同左 | ✅ 完全一致 |

`.review-workspace/fix-props/schema.json` 运行前后 SHA256 均为
`b71c2afde6c103132e4e1ca6b9896aab442bd80abbe1fcc0870f35ddce0f2019`，**未被改动**。

## 7. 结论与建议

**PASS 带非阻断。**

支持通过的依据：
1. Dev 的核心论证（M15b「挪到 `execute` 之后仍红」）**经本人实测成立**（killed 2/2）。
2. 上一轮 QA 指出的洞（M10/M11 存活、物理行序真变字典序却全绿）**已被修复**：本人复现 M10/M11 均 killed 2/2。
3. 产品面零风险：`src/` 零改动，全量 524 passed / 0 failed，产物 sha 逐字节一致。
4. 红线全部干净，三条性质断言与 humanize 断言可证未动。

非阻断发现（按严重度）：
- **[中] 确定性判据仍可被静默拆开，且已有 DB 取证。** M13（前人构造）+ M17 / M19（本人构造）
  三条存活路径都能让**物理行序真的变成字典序**，同时 8 条测试全绿。
  这正是 AC-004 立意要防的失效模式。**建议记入 follow-up，不阻断本轮** ——
  因为触发它必须有意改写自检机制本身，属测试可维护性问题，不是交付缺陷。
- **[低] M16 一类「recorder 撒谎」当前无害**，但证明 recorder 非可信证人；建议注释里点明。
- **[低] 物理行序在进程内原理性不可断言**，`ctid` 文本序不可靠（见 `:619-621`）。建议把「本判据不完备」写进设计文档。

建议的加固（可选，一行级改动）：把键先算一次，让 recorder 与 bind 共用同一个值 ——
`let kv: &str = …; pushed.push(kv); row.push_bind(kv)…`。
这能**堵住 M13 一类**（迭代值 ≠ 绑定值的缝），**堵不住 M17/M19 一类**（迭代层面的解耦）。
换言之：没有哪一行改动能封死这一类，剩下的只能靠「改动自检机制时必须同步复核」的纪律 + 文档说明。

## 8. 环境（代码失败 vs 环境失败，分开写）

- 集群：`172.16.120.20:30432`，库名 `ontol`（口令走环境变量，未写入本报告任何位置）。
- 观察到的**环境类**失败：复验开始时连续 2 次 baseline 全红，失败信息一律是
  `tests/common/mod.rs:66:10: 连不上测试库: Connect(PoolTimedOut)` ——
  **发生在建连接阶段、任何迁移之前**，且 TCP 端口可连（`/dev/tcp` 探测 OPEN），
  即服务器 accept 但未在 `acquire_timeout=10s`（`src/db/pool.rs:71-80`）内完成握手，符合共享集群被占满的特征。
- 同一 baseline 随后 3 次全绿（8/8）。**复现率随集群负载浮动，本报告不声称稳定值。**
- 基线计数：**baseline 3 绿 / 2 环境红（5 次尝试）**。
- 因此本报告的 runner 把「目标断言命中」与「环境失败」**分开判定**：
  只有命中 `历史行的插入顺序必须与 key 字典序相反` 等目标消息才计为 killed；
  `PoolTimedOut` / `Connection reset by peer` / `deadlock detected` 一律计入 `envfail`，**绝不冒充杀死**。
- 存活/杀死结论的每一轮均为 `envfail=0`（见 `all_results.txt`），故上述结论**不受集群抖动污染**。

## 9. 我自己引入又清掉的问题（如实报）

1. **`--test-threads=1` 导致首轮 baseline 全红**：我最初给 `cargo test` 加了串行标志，4 条测试
   `PoolTimedOut`。已去掉该标志（与前人口径一致）后基线转绿。**属我的操作失误，非产品缺陷。**
2. **M19 首次编译失败**：`pushed.push(*[_i,1,0][_i])` 对 `usize` 解引用（E0614）。
   已改为 `pushed.push(["m_mid","z_last","a_first"][_i])`，重跑后存活并取证。**编译失败未计入变异结果。**
3. **与在跑 runner 抢工作树**：我在 batch1 仍在跑 M19 时执行了一次 `apply.py M15`，短暂污染了工作树。
   事后**丢弃**了那批 M19 的 BUILDFAIL 结果并重跑，最终报告里的 M19 数据来自干净重跑。
4. **首轮误判口径**：我一度按简报表格以为该二进制有 5 条测试；实跑发现是 **8 条**，
   随即在报告 §0 更正，并把 runner 口径改为「轮数 N」与「单轮测试条数」分列。
5. **本环境无 `write` / `edit` 工具**：简报要求「含反引号的 Markdown 必须用 edit/write」，
   但本会话二者均不可用。**已按简报允许的兜底改用带引号 heredoc（`<<'MDEOF'`）写报告，并在此登记该偏离。**
6. **每次变异结束都 `git checkout --` 还原**，结束前复核：worktree `git status --short` 为空，HEAD = `9ff76e6`，
   热 target 符号链接 `verify-REQ-040/target -> code/target` **保持原样未删**。

## 10. 附录：完整变异表

二进制：`test_snapshot_write_paths-4119f8d8fad5cac3`（每轮 = 一次完整 `cargo test --test test_snapshot_write_paths`，内含 8 条测试）

| 变异 | 构造要点 | 单轮结果 | 杀死它的判据 / 存活论证 | 来源 |
|---|---|---|---|---|
| baseline | 未变异 | 8 passed | —（3 绿 / 2 环境红） | 本人 |
| baselineprobe | 只读回读（对照） | 8 passed | 回读 `is_dict_ordered=false` ⇒ 回读可信 | 本人 |
| **M10** | 反序副本喂 `push_values` | **7 passed; 1 failed** | **killed 2/2** @`:625` | 前人 + 本人复现 |
| **M11** | 排序副本喂 `push_values` | **7 passed; 1 failed** | **killed 2/2** @`:625` | 前人 + 本人复现 |
| **M15b** | 断言挪到 `execute` 后 + 反序切片 | **7 passed; 1 failed** | **killed 2/2** @`:625` ⇒ **Dev 论证成立** | 本人（跑完前人未跑完的） |
| M12 | 同切片、反向迭代 | 前人 0/5 @`:625` | killed（前人，未复核） | 前人 |
| M10b | M10 + 删早检查 | 前人 0/5 @`:625` | killed（前人，未复核） | 前人 |
| M2b | 调用点传字典序 | 前人 0/5 @`:625` | killed（前人，未复核） | 前人 |
| M5 | 只插 1 行 | 前人 0/5 @`:625` | killed（前人，未复核） | 前人 |
| M9 | 只插 2 行 | 前人 0/5 @`:756` | killed（原有前提断言，前人，未复核） | 前人 |
| M14 | 删掉 recorder 记录 | 前人 0/5 @`:683` | killed（前人，未复核） | 前人 |
| **M13** | bind 时置换 key | **8 passed（存活 2/2）** | **判据有洞**：物理行序 `is_dict_ordered=true` | 前人构造 + 本人取证 |
| M15 | 断言挪到 `execute` 后 | 8 passed（存活 2/2） | **等价变异**（只搬位置，未破坏顺序） | 前人 + 本人复跑 |
| M16 | recorder 撒谎但仍非字典序 | 8 passed（存活 2/2） | **等价变异**：回读 `is_dict_ordered=false`，前提完好 | 前人 + 本人复跑 + 取证 |
| **M17** | 独立 dry-run 填 recorder + 排序副本真插 | **8 passed（存活 2/2）** | **判据有洞**：`is_dict_ordered=true` | **本人新构造** |
| **M19** | recorder 按下标记固定序列 + 排序副本真插 | **8 passed（存活 2/2）** | **判据有洞**：`is_dict_ordered=true` | **本人新构造** |
| M16probe | M16 + 回读 | 8 passed | 物理行序仍 `false` ⇒ M16 等价的证据 | 本人 |
| M13probe | M13 + 回读 | 8 passed | 物理行序 `true` ⇒ M13 是洞的证据 | 本人（跑完前人未跑的） |
| M17probe | M17 + 回读 | 8 passed | 物理行序 `true` | 本人 |
| M19probe | M19 + 回读 | 8 passed | 物理行序 `true` | 本人 |
