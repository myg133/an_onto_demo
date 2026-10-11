# REQ-002 独立审核报告

> 审核者：Verifier（独立第三方视角，无作者自测结论背书）
> 审核对象：`/home/myg133/Projects/Anchnet/Ontology/feature-REQ-002/`，分支 `feature/REQ-002`，HEAD = `fa11bf7`

---

## 0. 审核元信息

### 0.1 范围

| 项 | 数量 |
| --- | --- |
| 被审代码 | `src/` 3,581 行 + `tests/` 2,468 行 = **6,049 行**（与任务书一致） |
| 逐行通读 | `src/domain/{hierarchy,review,view,apply,snapshot,diff}.rs`、`src/db/{apply,proposal,snapshot_store,pg,pool,project,config}.rs`、`src/cli.rs`、`tests/*` 全部 10 个文件 |
| 对照文档 | `acceptance.md`（15 条 AC 全文）、`design-summary.md`、`status.md`、`test-cases.md`、ADR-001、规格 `ontology-platform-spec.md`（定点查证 §4.6/§5.2/§8.3/§8.4/§9.1） |
| 判定器 | `BA/verify.sh` 150 行全文 + **7 个注入缺陷的实测** |
| 变异实验 | **19 个单变异 + 1 个组合变异**（19 个全部在独立 target 目录下重跑验证，见 §5.2） |
| 端到端 | 按 AC-012 文档命令序列实跑 + 绕过别名的库级复现 |

### 0.2 环境可用性

- **数据库：可达。** 外部 PG 16.14，经 `ONTOK_TEST_DATABASE_URL` 连接。口令全程未回显、未落盘到本目录之外的任何位置；报告中只出现变量名。
- **⚠ 一处环境偏差（须知）**：仓库本地 `.env` 里的库名是 `notol`，而服务端只有 `ontol`。直接用该文件会得到 `database "notol" does not exist`（我在首次 `cargo test` 中实测到 15/15 因连接失败）。我把库名改写为 `ontol` 后全部通过。`.env` 已被 `.gitignore:35` 忽略、不入库，故**不影响仓库**，但任何按该文件复现的人都会撞墙。
- **cargo test 实测：133 条，0 failed**（在 `/tmp` 副本上，库名改为 `ontol`）。与作者自述一致 —— 但见 §2 S6：我另外观察到 **1 次无法复现的偶发失败**。
- **`verify.sh` 实测：12 通过 / 2 未实现 / 0 未通过 / exit 0**，与作者自述一致。
- **无环境阻塞**：P0/P1 的所有检查都真跑了，没有一项是"因为环境所以没验"。

### 0.3 一句话结论

**有条件通过。** 领域逻辑与数据库层的实现质量**确实高于本项目过去的水平** —— 19 个变异里 15 个被杀，历史那两个 bug（命名空间混用、Pending 误判通过）都有判别力强的测试钉住，作者的复盘教训是真学到了。但有 **3 个阻断项**：`apply` 路径上的两道安全防线**完全没有测试覆盖**（同时失效仍 133 全绿）、判定器的 AC-002 红线判据**可被 `std::fs` 无声绕过**、以及 `--proposal p1` 别名**未按项目作用域解析**导致 AC-012 的文档命令序列在共享库上不可复现。

---

## 1. 阻断项（必须修才能合并）

### B1 · `apply` 路径的两道安全防线零测试覆盖，两层同时失效仍 133 全绿

**问题**
防止「悬空关系 / 层级环被写进不可变版本快照」有两道防线：

1. `Summarize.passed` 里的 `blocked.is_empty()`（`src/domain/review.rs:281`）
2. `apply()` 末尾的 `validate_hierarchy` 自检（`src/domain/apply.rs:169-176`）

**没有任何一条测试断言它们。** 单独去掉任一层，测试全绿；**两层同时去掉，15 条 apply 测试仍然全绿**。

**证据（变异实验，隔离 target 目录）**

| 变异 | 注入 | 结果 |
| --- | --- | --- |
| M22 | 删掉 `src/domain/review.rs:281` 的 `&& blocked.is_empty()` | **GREEN**，25 条 review 测试全通过 |
| M13 | 删掉 `src/domain/apply.rs:169-176` 整段层级自检 | **GREEN**，15 条 apply 测试全通过 |
| **C1（组合）** | **两个文件同时注入上述两个变异** | **`cargo test --test test_apply` → `test result: ok. 15 passed; 0 failed`** |

**影响**
`passed` 为 true 时，被判为 blocked 的条目会进入 `applicable` 并被真正 apply，而末尾自检本该兜住由此产生的悬空引用/环 —— 现在两道网都没有测试。AC-007「不产生野指针」与 AC-009「环必须被检出」在**领域层**有强测试（M04/M06/M07/M14 全部被杀），但它们在 **apply 落库路径上是空的**。

**为什么作者的自测没发现**
`test_apply_with_dangling_link_is_rejected` 存在且通过 —— 但它靠的是 `passed` 里的 `dangling_relations.is_empty()`（`review.rs:280`）。而 M05 证明**这一项是死条件**：`dangling` 与 `blocked` 在 `summarize` 里永远同时入列，所以真正承重的是 `blocked.is_empty()`，而它恰恰无人断言。这正是"作者自测掩盖了问题"的典型形态：有一条测试看起来在守这条线，实际守的是旁边那个冗余条件。

**建议**
```rust
// ① 构造 blocked 非空的提案，断言 summarize(...).passed == false
//    （不只看 blocked 列表内容）
// ② apply 成功后，断言 store.get_snapshot(v).parents() 的
//    validate_hierarchy 结果为空 —— 覆盖末尾自检
```

---

### B2 · 判定器 AC-002 判据可被真实 I/O 绕过（实验证实的假通过）

**问题**
`BA/verify.sh:86` 的 AC-002 判据只黑名单三个符号：

```bash
HITS="$(grep -rnE 'sqlx|PgStore|db::' src/domain/ | ...)"
```

`std::fs` / `std::net` / `std::process` / `tokio::fs` / 任意 HTTP 客户端**全部不在判据内**。

**证据（实验 G4）**
向 `src/domain/hierarchy.rs` 追加一个**能编译、不影响任何测试**的真实 I/O 函数：

```rust
pub fn audit_probe_io() -> Option<String> {
    std::fs::read_to_string("/etc/hostname").ok()
}
```

判定器输出：

```
AC-001 · 测试全绿
  ✓ cargo test 无失败（共 133 条）
AC-002 · 分层纪律：src/domain 零 I/O
  ✓ src/domain/ 无 I/O 符号            ← 真实 I/O 已存在，仍判通过
统计：通过 12 · 未实现 2 · 未通过 无
GATE|G4|exit=0
```

**影响**
AC-002 是本项目唯一的架构红线判据，也是 `AC-002 判据本身够不够` 这个问题的答案：**不够，且缺口正好覆盖最常见的 I/O 写法**。红线可以被无声突破，判定器还给出全绿。

**建议**
判据扩为 `sqlx|PgStore|db::|std::(fs|net|process|io|env)|tokio::fs|reqwest|ureq|hyper|Command`；更稳的做法是换成**编译期约束** —— 把 `domain/` 拆成独立 crate 且依赖里不出现任何含 I/O 的 crate，让违反直接编译不过，而不是靠 grep。

---

### B3 · `--proposal p1` 未按项目作用域解析，审核动作可能落在别的项目上

**问题**
`resolve_proposal` 与 `proposal_alias` 都是**全库**排序取第 n 个，没有 `project_id` 过滤：

- `src/cli.rs:517` — `SELECT id FROM proposals ORDER BY created_at DESC, id DESC`
- `src/cli.rs:536` — 同上
- `src/cli.rs:534` — `async fn proposal_alias(pool: &PgPool, _project_id: Uuid, id: Uuid)`，**参数被下划线标注为未使用**

那个 `_project_id` 就是铁证：作者知道该按项目限定，参数都传进来了，却没写进 SQL。

**证据（按 acceptance.md 的 AC-012 文档序列实跑）**

```
$ ontok propose import --file fixtures/proposal-demo.json
提案 p1 已导入，共 6 个条目： [1] 客户 … [6] 资产→设备      ← 导入成功
$ ontok review decide --proposal p1 --item 3 --track biz --accept
✗ 提案里没有第 3 个条目                                      ← 解析到了别的提案
$ ontok review show --proposal p1
提案 p1 状态=applied
  [1] y   add_entity_type biz=accepted model=accepted Passed  ← 1 条目、非我导入、已 applied
```
12 条 `review decide` 中 **9 条失败**。随后连续两次 `import`，`p1` 指向的提案又各不相同。

**影响**
1. **AC-012（本次 MVP 头条判据）的文档命令序列在共享外部库上不可复现** —— 而 AC-001 的前置条件写明"外部 PostgreSQL"是既定方案。
2. 更严重的是 `review decide` 会把审核决定**写进别的项目的提案**，且照样"成功"。

**这是作者已知 bug ② 的同一个根因（"作用在别人的数据上"）的另一半**：那次只修了**顺序**（正序→倒序），没修**作用域**。代码注释 `src/cli.rs:507` 自己写着"测试库里躺着几百条历史提案…演示命令作用在别人的数据上" —— 修复只覆盖了历史数据，没覆盖**并发写入**与**跨项目**。

**建议**
两处 SQL 改为 `SELECT id FROM proposals WHERE project_id = $1 ORDER BY created_at DESC, id DESC`（`resolve_proposal` 需把 project 传进来），并启用 `proposal_alias` 的 `_project_id`。

---

## 2. 严重问题（应修，不阻断合并）

### S1 · `apply` 的 base 冻结闸门 fail-open
`src/db/apply.rs:58`：`if projects.is_frozen(...).await.unwrap_or(false) {`

查询出错时 `unwrap_or(false)` 把错误当成「未冻结」放行。紧随其后的 `get_snapshot` 是**另一次**查询，可以成功，于是会在**已冻结的 base** 上建出新版本 —— AC-006 的不变量在故障态下被绕过。
爆炸半径有限（冻结快照本身不会被改写，因为 `save_snapshot` 有独立闸门，且 M11 证明那个闸门被测到了），但方向是错的。
**建议**：改 `.await?`，让它随错误一起失败（fail-closed）。

### S2 · 拒绝语义在整条集成链路上零覆盖
变异 M18：删掉 `parse_decision` 的 `"rejected" => Decision::Rejected` 分支（`src/db/proposal.rs:257`），
`cargo test --test test_apply` → **15/15 全绿**。
阳性对照：同一变异跑 `test_proposal` → 被 `test_proposal_unknown_decision_is_pending_not_accepted` 杀死。

15 条 apply 测试**没有一条写入过 `Rejected`**；`test_proposal_set_decision_per_track`（`tests/test_proposal.rs:118-133`）只测 Accepted。
**影响**：若某次重构让"拒绝"被静默解析成 Pending，apply 层完全无感。而 G-5（§7）恰好使"拒绝"成为当前唯一有语义争议的状态 —— 覆盖缺口与已知缺陷叠在同一处。
**建议**：加集成测试 —— 建模轨 `Rejected` → `entry_state == Vetoed`；跨轨场景（否决后另一轨 Accepted）也补一条（跨轨实现见 `review.rs:161-164`，是有的，只是没测）。

### S3 · `decisions_log` 留痕是不实注释 —— G-5 的唯一说辞不成立
`src/db/proposal.rs:181` 注释称"当前实现**允许**（后写覆盖），并在 `decisions_log` 留痕"。

全仓 grep `decisions_log` **只命中这一行注释**。`src/db/pg.rs` 建的 8 张表里没有它；`set_decision` 是 `UPDATE proposal_items SET {col} = $2 WHERE id = $1`（`proposal.rs:195`），**单列原地覆盖，无历史**。

**影响**：G-5 的严重性被作者低估 —— 不是"允许抹掉但有审计"，而是**抹掉后无从追溯**：一次显式否决被同轨 Accepted 覆盖后，库里不存在任何记录表明它曾被否决过。这使 G-5 从"待拍板的语义问题"升级为"审计缺口"。

### S4 · 判定器删掉红线测试文件只降级为「未实现」，仍 exit 0
实验 G2：删除 `tests/test_no_write_path.rs`（AC-004 安全底线的**全部**载体）→
```
  ⚠ tests/test_no_write_path.rs 尚未创建（对应任务未开始）—— 判为「未实现」
统计：通过 11 · 未实现 3 · 未通过 无
GATE|G2|exit=0
```
"三态区分"在任务未开工时是合理设计，但对一个**已完工**的 MVP，它是一条现成的假通过通道：AC-004 唯一测试被删掉后，判定器仍宣布"自动判定项无未通过"。
**建议**：引入阶段标志（完工后 文件缺失 = ✗ 而非 ⚠），或对每个红线测试同时断言"存在 + 通过"。

### S5 · 判定器对 AC-008 完全没有判据，且 7 条 AC 无独立判据
`verify.sh` 全文 grep 到的 AC 编号只有 **001/002/003/004/009/012/013/014 共 8 条**。
`verify.sh:116-127` 的 require_test 清单 8 项，覆盖 004/005/006/007/010/011/014/015 —— **独缺 AC-008**，而 acceptance.md 为 AC-008 明确指定了判据测试 `test_chained_apply_keeps_accumulating`（该测试确实存在于 `tests/test_apply.rs:157` 且通过）。

所以作者宣称的"判定器 12 通过"**不等于**"12 条 AC 各自被判定"：AC-005/006/007/008/010/011/015 中有 7 条没有独立判定行（只被 AC-001 的 `cargo test` 兜住 —— 兜底是有效的，但"判定器逐条判了 AC"的说法不成立）。

### S6 · 测试套件不封闭：与外部共用一个库，无隔离/串行化
证据（我审核期间亲历）：
- 按 AC-012 文档实跑时，`p1` 两次解析到**我没有创建过**的提案（见 B3）。
- `ontok project list` 中存在他人产物 `acme_demo_1790779668`。
- `tests/common/mod.rs` 只用 `t_{前缀}_{pid}_{n}` 命名项目，**不清理、不串行**；15 个测试并发各持一个 `max_connections(5)` 的池（`src/db/pool.rs:28`）。

**附带观察（存疑）**：我在**首次**全量 `cargo test` 时观察到 `test_apply_remove_entity_type_cleans_links` 失败 1 次，随后 **3+8=11 次**重跑全部通过，**未能复现，且当时未捕获失败输出**，根因未定。与"连接争用 / 并发 DDL（`migrate()` 被 15 个测试并发调用）"的假设一致，但**未证实**，我不把它算作确认缺陷。

**影响**：「133 全绿」作为验收信号不可靠 —— 它同时取决于同库的其他租户。
**建议**：每 CI 任务用独立 schema；或 DB 测试串行化；`migrate()` 加 advisory lock。

### S7 · 判定器在编译失败时，AC-001 给出空洞的 ✓（实验 G1）
注入 `compile_error!` 后：
```
AC-001 · 测试全绿
  ✓ cargo test 无失败（共 0 条）      ← 零条测试运行，却判「无失败」
统计：通过 3 · 未实现 2 · 未通过 有
GATE|G1|exit=1
```
整体 exit 仍为 1（靠后面 8 条 require_test 兜住），所以**今天还不是完整假通过**。但 `verify.sh:73` 的 `grep -qE '^test result: FAILED'` 在无 `test result:` 行时必然落空 → 走 else 分支。最关键的一项在最坏情况下报空洞的绿灯；若日后有人删减那 8 条 require_test，这一条立刻变成完整假通过。
**建议**：AC-001 额外断言 `N > 0` 且 `cargo` 退出码为 0。

---

## 3. 一般问题 / 改进建议

| # | 问题 | 证据 | 建议 |
| --- | --- | --- | --- |
| G-1 | **判定器本身没有任何测试**（任务书点名的 P0 级流程风险） | 全仓无任何测试引用 `verify.sh`；它只出现在 5 个 `.md` 里 | 见下方专门说明 |
| G-2 | `cli.rs` 文件头与实现的别名语义**自相矛盾** | `src/cli.rs:9`「`p1` = 该项目**最早**的第 1 个提案」 vs `src/cli.rs:502/507-510` + SQL「倒序/最新」 | 删掉过时的文件头注释（9 行） |
| G-3 | JSONB 往返错误被静默吞成 `None`，审计字段可能无声丢失 | `db/proposal.rs:102`、`db/snapshot_store.rs:158,181`、`db/apply.rs:70` 均用 `.ok()` | `extracted_by` 反序列化失败改为报错 |
| G-4 | 生产代码里的 `unwrap()` | `src/cli.rs:454`、`src/db/snapshot_store.rs:213` | 实际不可失败，但属坏味道，改 `?` |
| G-5 | `let _ = locked;` 把行锁当语句用 | `src/db/apply.rs:93-95` | 加注释说明"只为拿锁" |
| G-6 | `parse_kind` 未知值落到 `AddProperty` | `src/db/proposal.rs:250` | 方向保守且有注释论证，**可接受**，仅记录 |
| G-7 | `passed` 里 `dangling_relations.is_empty()` 是死条件 | M05 存活；`blocked` 与 `dangling` 永远同时入列 | 保留无害（防御式冗余）。**但真正承重的 `blocked.is_empty()` 恰好无覆盖 → B1** |
| G-8 | 运行时 CLI 会回落到"测试专用"连接变量 | `src/cli.rs:178-179` `require_env(DATABASE_URL_ENV).or_else(TEST_DATABASE_URL_ENV)`；`config.rs:9` 标注该变量"**测试专用**" | 生产 CLI 静默连测试库有风险，建议只认 `ONTOK_DATABASE_URL` |
| G-9 | `.env.example` 里**一个变量名都没有** | 该文件仅 2 行（1 行注释 + 字面量 `.env`，共 41 字节），而 `config.rs:4` 声称"`.env.example` 只写变量名"、测试 panic 信息让用户"见 .env.example" | 补上变量名 |
| G-10 | 本地 `.env` 库名写错（`notol` vs 实际 `ontol`），且含一行裸 `.env` 导致 `source` 报 command not found | 见 §0.2 | 修正；该文件已 gitignore，不影响仓库 |
| G-11 | CLI 的 `Snapshot → SnapshotView` 映射**无测试** | 映射在 `src/cli.rs:432-468`；`view::project` 仅被 `cli.rs:471` 与 `test_view.rs`（手工构造 fixture）调用 | 补一条"apply 产出的快照 → 客户视图"集成测试。**好消息**：`cli.rs:473-478` 有运行时 `scan_modeling_terms` 兜底，泄露即 `exit 2` |
| G-12 | base 快照缺失时误报 `BaseVersionFrozen` | `src/db/apply.rs:61-64` | 换专门的 `BaseSnapshotMissing` |
| G-13 | 环检测对环上每个节点各报一次 | `hierarchy.rs` 校验逐节点调用 | 仅影响错误信息可读性 |
| G-14 | `test_proposal_unknown_decision_is_pending_not_accepted` 用 `#[tokio::test]` 但不碰 DB | `tests/test_proposal.rs:186-196` | 移到纯域侧；会让人误判"哪些测试真需要库" |

**已确认安全的删除**：`src/db/ontology.rs`（提交 `fa11bf7`）—— `src/db/mod.rs` 从未声明该模块，全仓 grep `db::ontology` / `mod ontology` **零命中**，该文件仅 1 行。**删除是安全的。**

### G-1 专述：判定器没有测试

作者自己写过"判定器本身也是代码，也要有测试"（`acceptance.md` AC-002 注释区）。**事实是没有。**

我在审核中临时补了 4 个用例来验证判定器（结果见 §6）。这 4 个用例里有 **2 个判定器没抓住**（G4 的 I/O 绕过、G2 的删红线文件）。建议把这 4 个固化下来：

| 注入 | 期望 | 实际 |
| --- | --- | --- |
| 往测试里注入一条必失败的断言 | exit ≠ 0 | ✅ exit=1（守住） |
| 把红线测试改名（零匹配） | exit ≠ 0 | ✅ exit=1（守住） |
| 往 `domain/` 注入 `std::fs` I/O | exit ≠ 0 | ❌ **exit=0** |
| 删掉 AC-004 红线测试文件 | exit ≠ 0 | ❌ **exit=0** |

固化成 `BA/verify_selftest.sh`，每次改判定器必跑。

---

## 4. AC 逐条结论表

| AC | 声明 | 我的结论 | 证据 | 备注 |
| --- | --- | --- | --- | --- |
| AC-001 | 测试全绿 | **有条件通过** | 判定器沙盒复跑 `cargo test` → 133 条 0 failed；但观察到 1 次未复现的偶发失败，且套件与他人共用库 | 见 S6 |
| AC-002 | 领域层零 I/O | **通过（判据不通过）** | 代码：6 个 domain 文件通读，无 I/O；但判定器判据经实验 G4 可被 `std::fs` 绕过 | 见 B2 |
| AC-003 | 快照契约含全部占位 | 通过 | 判定器 ✓ 9 项齐全；`snapshot.rs` 确有 `WriteBinding`(:115)、`effects`(:145)、`extracted_by`(:173)、`action_types`(:180)、`functions`(:182)、`disjoint_with`、`failed_groups` | — |
| AC-004 | 切片 1 无可执行写回路径 | **通过**（依据代码审计，非依据测试） | 全部 SQL 仅写本体自有 8 表（`db/pg.rs` 建表清单）；`WriteBinding` 只有结构无执行器（`apply.rs:140` 仅 `retain` 清理）；CLI 无写回子命令 | 承载它的 2 个测试很弱，见 §5.1 |
| AC-005 | 客户视图零建模术语 | **通过**（唯一双保险） | `test_view_biz_view_never_exposes_modeling_terms` 通过；M08/M09/M16 三个回归变异**均被杀**；AC-012 实跑客户视图命中 `[]`；且 `cli.rs:473-478` 运行时兜底 | 质量最高的 AC |
| AC-006 | 冻结后拒绝一切写入 | 通过（1 处 fail-open） | M11、M12 **均被杀**（`test_frozen_version_rejects_snapshot_write` / 2 条）；AC-012 实跑确认 `BaseVersionFrozen` | 见 S1 |
| AC-007 | 关系条目依赖两端已被接受 | 通过 | `test_review_link_acceptance_requires_both_ends_accepted`；M04/M06 被杀；实跑"单轨接受关系 → `blocked 1, dangling 1`"被拒 | 承重条件无覆盖 → B1 |
| AC-008 | 链式 apply 累积而不重置 | 通过（测试存在且通过） | `tests/test_apply.rs:157 test_chained_apply_keeps_accumulating` 通过 | 判定器里没有它 → S5 |
| AC-009 | 环检测与悬空父引用 | 通过 | M07（环检测失效）被 **4 条**测试杀；M14（写入前环检查恒 false）被 **3 条**杀 | apply 落库路径上的环拦截无断言 → B1 |
| AC-010 | 快照往返无损（含 None / 空列表 / 空 JSON 三态） | **通过** | `test_snapshot_roundtrip` 真实覆盖三态（`None` / `Some("")` / 空 JSON 三者分别断言） | **声明与测试一致，无虚报** |
| AC-011 | `extracted_by` 记录 agent 与其具体版本 | 通过 | `test_snapshot_records_extracted_by` 通过 | — |
| AC-012 | 端到端 16 条命令 | **未通过** | 文档序列实跑：12 条 decide 中 9 条失败（`✗ 提案里没有第 3 个条目`），`p1` 解析到非我导入的提案。改用显式 UUID 后**全部子判据通过**：部分审核被拒 / 单轨关系被拒 / v2=4 对象+2 关系 / `设备.parent=asset` / 冻结后 `BaseVersionFrozen` / diff=6 新增 / 客户视图零术语 | 功能实质正确，**别名作用域**使其不可复现 → B3 |
| AC-013 | 3 名外部评审人判读客户视图 | **需人工 / 未执行** | 我不是外部评审人，无法也不能代判；判定器同样标 ⚠「需外部评审人，缺则判『未执行』而非『通过』」 | 不得由开发者代替 |
| AC-014 | `extracted_by` 按 ADR-001 取值 | 通过 | M03（改为继承 base）被 **2 条**测试杀：`test_extracted_by_is_not_inherited` / `..._on_apply` | — |
| AC-015 | `failed_groups` 不继承 | 通过 | `test_failed_groups_not_inherited` 通过 | — |

**汇总：通过 11 · 有保留通过 2（AC-001/AC-002）· 未通过 1（AC-012）· 需人工 1（AC-013）**

---

## 5. 测试有效性审查

### 5.1 无判别力 / 弱判别力的测试

**① `test_no_write_path_in_slice1` — 判别力≈0**（`tests/test_no_write_path.rs:19-43`）
只黑名单 5 个函数名片段：`fn execute` / `fn write_back` / `fn dispatch` / `fn run_action` / `fn apply_effect`。
任何叫 `fn sync_to_erp` / `fn push_rows` / `fn write` 的执行器都能通过。
**但模块注释（:5-9）主动声明了"无法穷尽证明、只做已知入口清单、不声称完备"** —— 这个诚实度值得肯定，也让我把它降级为"文档化清楚的弱测试"而非"虚假保障"。AC-004 的真正依据是我的代码审计，不是这个测试。

**② `test_store_connects_to_exactly_one_database` — 典型的假保障**（`tests/test_no_write_path.rs:69-91`）
它用 `pool.clone()` 构造 4 个 store，然后断言它们的连接目标等于**同一个 pool** 的 `current_database()`。这是在断言**测试自身的构造方式**，不经过任何生产建连路径。即使生产代码能另开一个指向客户库的连接，这个测试照样通过。看起来像强架构不变量，实际是空转。

**③ diff 的 Added/Removed 分支只断言类型不断言内容**（`tests/test_diff.rs:73-96`）
`test_diff_added_entity` / `test_diff_removed_entity` 只断言 `kind` 与 `target`，不断言 `before`/`after` 的内容 → M17 存活（见 §5.2）。

**④ 一个纯函数测试混在需要数据库的文件里** — `tests/test_proposal.rs:186`（见 G-14）。无害，但会误导"哪些测试真需要库"的判断。

**⑤ 作者的假命题教训已修好（正面结论）**
任务书点名的 `descendants_exclude_self` 假命题测试，**作者确实改了**：现在的
`test_hierarchy_descendants_exclude_self`（`tests/test_hierarchy.rs`）用 3 层树 + 期望精确有序结果 `["机器人设备","设备"]`，
而 M01（去掉 `seen.insert(key)` 的自身预置）在**环场景**下才发作，被新增的 `test_hierarchy_descendants_exclude_self_under_cycle` 杀死。
**这个复盘教训是真的学到了，不是嘴上说说。**

**⑥ 与 acceptance.md 声明的一致性抽查**
- AC-010 声明"含 None / 空列表 / 空 JSON 三态" → 测试**真的**分别断言了三者（`test_snapshot_roundtrip`），无虚报。
- AC-008 声明的判据测试存在且通过 → 无虚报。
- **但判定器的 8 条 require_test 每条只跑 1 个测试名**（输出 `通过（1 passed）`），意味着"判定器判了 AC-005"实际等于"只跑了 AC-005 的 1 条测试"。真正兜住回归的是 AC-001 的全量 `cargo test`。

### 5.2 变异验证结果

**方法学（必须先说，因为它影响结论可信度）**
我第一版 harness 让所有变异**共用一个 `CARGO_TARGET_DIR`**，导致陈旧 `libontok` rlib 被链接 —— 同一个变异 M18 在 `test_apply` 上报 GREEN、在 `test_proposal` 上报 RED，**互相矛盾**。
自查发现后废弃 v1，改为**每个变异一个全新 target 目录**（`mutate2.sh`），并跑了双向对照：

| 对照 | 变异 | 目标 | 结果 |
| --- | --- | --- | --- |
| 阳性 | M18（删 `rejected` 分支） | `test_proposal` | **RED** ← 变异确实生效 |
| 阴性 | M03（`extracted_by` 继承） | `test_apply` | **RED** ← 结果方向正确 |
| 独立复核 | 纯净隔离构建 `parse_decision("rejected")` | — | `Rejected`（确认 v1 的 `Pending` 是污染，非产品缺陷） |

下表 **19 个变异全部在隔离 target 目录下重跑**。

| 变异 | 注入的缺陷 | 目标测试 | 结果 | 杀死它的测试 |
| --- | --- | --- | --- | --- |
| M01 | `descendants_of` 不预置自身 → 把自己算成后代 | test_hierarchy | **RED** | `test_hierarchy_descendants_exclude_self_under_cycle` |
| M02 | `summarize` 用 `target` 展示名而非 `type_key`（**历史 bug ①**） | test_review_state | **RED** | `test_review_available_uses_type_key_not_display_target`, `test_review_remove_uses_type_key_namespace` |
| M03 | `extracted_by` 继承 base（**违反 ADR-001**） | test_apply | **RED** | `test_extracted_by_is_not_inherited`, `..._on_apply` |
| M04 | `entry_state` 把 `Pending` 当通过（**历史 bug ②**） | test_review_state | **RED** | **6 条**（含 `..._pending_item_blocks_overall_pass`） |
| M05 | `passed` 不检查 `dangling_relations` | test_review_state | 🟡 **GREEN** | — （**等价变异**，见下） |
| M06 | `available` 不扣减被删类型（**历史 bug**） | test_review_state | **RED** | `test_review_removed_type_still_blocks_link_to_it` 等 **3 条** |
| M07 | `find_cycle_from` 检不出环（**历史 bug**） | test_hierarchy | **RED** | `test_hierarchy_detects_cycle` 等 **4 条** |
| M08 | `humanize_column` 认不出就原样透出（S-2 回归） | test_view | **RED** | `test_view_drops_property_when_column_unhumanizable` |
| M09 | 守门词扫描变大小写敏感（S-3 回归） | test_view | **RED** | `test_view_blacklist_scanner_is_case_insensitive` |
| M10 | 删类型时不清理关系（留野指针） | test_apply | **RED** | `test_apply_remove_entity_type_cleans_links` |
| M11 | `save_snapshot` 去掉冻结闸门 | test_snapshot | **RED** | `test_frozen_version_rejects_snapshot_write` |
| M12 | `apply` 的 base 冻结检查恒为未冻结 | test_apply | **RED** | `test_apply_on_frozen_base_is_rejected` 等 **2 条** |
| M13 | `apply` 末尾 `validate_hierarchy` 自检被去掉 | test_apply | 🟡 **GREEN** | — → **B1** |
| M14 | `would_create_cycle` 恒 `false` | test_hierarchy | **RED** | `test_hierarchy_would_create_cycle_on_descendant` 等 **3 条** |
| M16 | 客户视图不防 key 泄露（S-1 回归） | test_view | **RED** | `test_view_never_publishes_key_as_display_name` |
| M17 | diff 把 `evidence` 算进去 | test_diff | 🟡 **GREEN** | — |
| M18 | `parse_decision` 忽略 `rejected` | test_apply | 🟡 **GREEN** | — → **S2** |
| M20 | `type_key` 忽略 `payload.key` | test_review_state | **RED** | **3 条** |
| M22 | `passed` 不检查 `blocked` | test_review_state | 🟡 **GREEN** | — → **B1** |
| **C1** | **M22 + M13 同时（两层防线全失）** | test_apply | 🟡 **GREEN** | — → **B1** |

**15/19 变异被杀。** 4 个存活者的定性：

- **M05（等价变异，非盲区）**：`dangling_relations` 与 `blocked` 在 `summarize` 里永远同时入列（同一分支里一起 push），所以删掉 `dangling_relations.is_empty()` 是语义无操作。**不是测试的锅**，但它揭示了真正承重的是 `blocked` —— 而那个没有测试（B1）。
- **M13 / M22 / C1**：**真盲区**，见 B1。已实测确认两层同时失效后 `test_apply` 仍 `15 passed`。
- **M17（窄盲区）**：`entity_json` 只用于 Added/Removed 的整块 payload；`test_diff_ignores_evidence_entirely` 只走 Modified 路径（`entity_json` 不参与），所以把 `evidence` 塞进 `entity_json` 无人发现。严重度低（只是 diff 输出多了噪声，不影响"evidence 变化不产生 diff"这条主契约），但"evidence 不进 diff"这个**声明**只被验证了一半。
- **M18（真盲区）**：见 S2。

### 5.3 覆盖率缺口

1. **`Summarize.passed == false` 从未在 blocked 场景被断言**（B1，最高优先级）
2. **apply 产出的快照的 `validate_hierarchy` 为空，从未被断言**（B1）
3. **拒绝（`Rejected`）语义在 apply/store 集成链路上零覆盖**（S2）
4. **CLI 零测试**：别名解析（B3 就是这么漏的）、`propose import` 的字段校验、`version freeze`、`ontology diff` 输出格式 —— 全部无自动化测试，AC-012 是唯一保障且是人工的
5. **CLI 的 `Snapshot → SnapshotView` 映射无测试**（G-11）—— 好在有运行时守门
6. **diff 的 Added/Removed payload 内容无断言**（M17）
7. **判定器自身零测试**（G-1）

---

## 6. 判定器（`BA/verify.sh`）审查

### 6.1 作者声称修好的误判，核实结果

| 作者声称（`verify.sh:4-8`、:38-44） | 我的核实 | 结论 |
| --- | --- | --- |
| ① 测试名与实际不符 → 零匹配却退 0 | 实验 G3：把 `test_no_write_path_in_slice1` 改名 → `✗「test_no_write_path_in_slice1」零匹配 —— 判定失败（不是通过）`，exit=1 | ✅ **真修好了** |
| ② grep 匹配到文档里写的命令本身 | 判定器已不再 grep `acceptance.md`，改为硬编码测试名 | ✅ 已消除该类 |
| ③ `\|\| echo ✓` 吞掉退出码 2 | `require_test` 已不用 `\|\| echo ✓`；文件不存在走独立的 `warn` 三态分支 | ✅ 已消除该类 |
| ④（第 6 次）`pipefail` + `grep -q` 提前退出导致假通过 | 实验 G7：注入必失败断言 → `✗ 存在失败测试`，exit=1 | ✅ **真修好了** |
| ⑤ 目录不存在 → 判定无效 | 三个分支都有 | ✅ 结构到位 |

**结论：作者点名的 4 类误判确实修好了，7 次误判的自述可信。**
（另有 §1 B2 / §2 S4 / S7 三条**新**的假通过通道，是这几类之外的新问题。）

### 6.2 仍存在的假通过 / 假失败

| 类型 | 触发条件 | 判定器行为 | 实测 |
| --- | --- | --- | --- |
| **假通过** | `domain/` 引入 `std::fs`/`std::net`/`tokio::fs` 等 I/O | `✓ 无 I/O 符号`，exit=0 | **G4 已证实** → B2 |
| **假通过** | 删除任一红线测试文件 | `⚠ 未实现`，exit=0 | **G2 已证实** → S4 |
| **假通过（局部）** | 代码编译失败 | AC-001 `✓ cargo test 无失败（共 0 条）` | **G1 已证实**（整体 exit=1） → S7 |
| 假失败（推断） | 源码行尾注释里出现 `sqlx`（如 `let x = 1; // 见 sqlx::query`） | `verify.sh:86` 的过滤器只排除**行首**注释，行尾注释会被算作 I/O 符号 → 误报 ✗ | **未实测**，标为**存疑**（判读代码所得） |
| 假失败（推断） | 字符串字面量里含 `db::` / `sqlx` | 同上 | **未实测**，存疑 |

### 6.3 AC 覆盖完整性

- `verify.sh` 出现过的 AC 编号：**001 / 002 / 003 / 004 / 009 / 012 / 013 / 014 = 8 条**
- `require_test` 覆盖：004 / 005 / 006 / 007 / 010 / 011 / 014 / 015（**独缺 008**）
- **无独立判据的 AC：005 / 006 / 007 / 008 / 010 / 011 / 015 共 7 条** —— 全部只被 AC-001 的全量 `cargo test` 兜底。兜底是有效的（M12/M11/M03 等变异都是靠它被杀），但"判定器逐条判了 15 条 AC"的说法不成立。
- AC-008 特别说明：acceptance.md 为它指定了判据测试 `test_chained_apply_keeps_accumulating`，测试确实存在且通过，**只是判定器漏了这一行**。→ S5

### 6.4 判定器有没有测试

**没有。** 全仓无任何测试/脚本引用 `verify.sh`（只出现在 5 个 `.md` 里）。作者在 `acceptance.md` AC-002 注释区写过"判定器本身也是代码，也要有测试"，**这一条没有兑现**。

我在审核中临时补了 4 个用例，**2 个没抓住**（B2、S4）。建议固化为 `BA/verify_selftest.sh`。→ G-1

---

## 7. 未决问题评估

### G-5：同轨 `Accepted` 能否抹掉显式 `Rejected`？

**作者描述**：代码已标注"需加守卫"但没加，文档写"显式否决永远有效"，实现是"后写覆盖"。

**我的确认**：**行为确实存在。**
`src/db/proposal.rs:183-202 set_decision` 对 `Track::Biz`/`Track::Model` 都执行同一条
`UPDATE proposal_items SET {col} = $2 WHERE id = $1`，**没有任何 `WHERE col <> 'rejected'` 前置条件**。
实测（我在 `/tmp` 副本加的探针）：
```
写 Rejected 后:  biz='pending'  model='rejected'
同轨再写 Accepted 后: model='accepted'      ← 显式否决被抹掉
```

**但作者对严重性的描述不完整 —— 这一点比 G-5 本身更重要：**
`proposal.rs:181` 声称"并在 `decisions_log` 留痕"。**`decisions_log` 不存在** —— 全仓只在那一行注释里出现；`db/pg.rs` 的 8 张表里没有它；`set_decision` 是**单列原地覆盖，无事件历史**（S3）。

所以现状不是"允许抹掉，但有审计可追"，而是：**抹掉后库里不存在任何记录表明它曾被否决过。**

**影响面（我要把话说准，避免夸大）**
- **跨轨是安全的**：`entry_state`（`review.rs:161-164`）只要任一轨 `Rejected` 就返回 `Vetoed`，另一轨的 `Accepted` 盖不掉它。设计文档"显式否决永远有效"在跨轨场景**已实现**。
- **同轨是漏洞**：建模轨先否决、再被同轨接受 → 否决消失；若另一轨也已接受，条目直接变 `Passed`，`summarize.passed` 为 true，**apply 成功**。也就是说"建模人员明确否决的东西"可以被无声放行。
- **可达性**：需要评审人（或 CLI）在同一轨上先 `--reject` 后 `--accept`。`cli.rs:320-321` 只校验 `--accept`/`--reject` 二者恰给一个，不校验与当前状态的关系 → 极易发生。
- **与覆盖缺口叠加**：M18 证明 apply/store 集成层对"拒绝"零覆盖（S2）。**当前唯一有语义争议的状态，恰好也是唯一没有集成测试的状态。**

**是否应阻断合并？我的判断：应阻断，或至少"必须先拍板 + 落审计"再合。**
理由：这是本项目**唯一**的核心业务不变量（"显式否决永远有效"）被实现违反，而实现侧的注释还基于一个不存在的审计机制。文档与实现在此处**直接矛盾**，且矛盾方向是安全方向的反面。
最低要求（三选一，由 BA/产品拍板）：
1. 加守卫：同轨 `accepted` 命中 `rejected` 时报错拒绝（最小改动，符合设计文档）；
2. 或明确裁定"同轨可覆盖"，但**先把 `decisions_log` 落下来**（当前是"可覆盖且无痕"，不可接受）；
3. 无论选哪个，补集成测试（S2）把这个状态钉死。

**存疑部分**：我不掌握产品侧对"改主意"场景的真实需求（可能确有"先否决后改判"的合法流程）。这是**决策问题，不是技术问题**，我不替 BA 拍板 —— 但我确认代码当前**与设计文档矛盾**，且注释所依赖的审计机制不存在。

### 其它未决项
- §2 S6 的一次偶发失败：现象确认、**根因未确认**，需要开着输出重跑到复现才能定性。
- §6.2 的两条假失败：代码判读所得，**未实测**，标存疑。

---

## 8. 我没能验证的部分

| 项 | 状态 | 原因 / 需要什么 |
| --- | --- | --- |
| **AC-013**（3 名外部评审人判读客户视图） | **未执行** | 这不是我能做的事。需 3 名**非本次开发者**的评审人执行 `ontok ontology show --version 2` 并回答两问，≥2/3 通过才算通过。判定器已正确标 ⚠。**不得由开发者或审核者代判。** |
| **AC-012 的 16 条 CLI 命令原样复现** | **部分** | 功能实质我用库级调用全部验证通过（见 AC 表）。但**原样文档序列无法复现** —— 因为 B3（别名未按项目作用域）+ 共享库并发写入。我**没有**在干净私有的库上重跑文档序列（我没有权限也没有理由新建/清空库）。要完全验证，需在一个**独占的空库**上按文档逐字执行。 |
| §2 S6 偶发失败的根因 | **未定性** | 观察到 1 次 `test_apply_remove_entity_type_cleans_links` 失败，之后 11 次重跑全过，**当时未捕获失败输出**。需要开着完整输出反复重跑全量套件才能抓到 message。假设（连接争用 / 并发 DDL）**未证实**。 |
| §6.2 两条"假失败"（行尾注释 / 字符串字面量含 `sqlx`） | **未实测** | 结论由 `verify.sh:86` 的过滤器逻辑判读得出。我**没有**实际注入这两类输入跑判定器（时间预算给了 B2/S4/G1）。需要时我可以补做。 |
| 设计规格 `ontology-platform-spec.md`（92KB） | **定点查证** | 未逐行通读。只按 AC 声明的对应章节（§4.6 / §5.2 / §8.3 / §8.4 / §9.1 / §2.2 / §2.3 / §3）核对实现。**若某条 AC 的真实规格在别的章节，本次审核不会发现。** |
| `test-cases/test-cases.md` | **抽样比对** | 未逐条比对全部测试用例与实现；只抽查了 AC-010 三态、AC-008 判据测试两处"声明 vs 实际"的一致性。 |
| 非 REQ-002 的历史提交 | **未审** | 只审 `feature/REQ-002` @ `fa11bf7` 的工作树状态（6,049 行）。未做跨 commit 的历史回归审计。 |
| 客户真实数据 | **未接触** | 全程只连 `ONTOK_TEST_DATABASE_URL` 指向的专用测试库。未连、未查任何其他数据库。 |

**审查纪律声明**：本次审核**未修改** `/home/myg133/Projects/Anchnet/Ontology/` 下的任何文件；**未执行任何 git 写操作**（只用了 `log` / `status` / `ls-files` / `check-ignore` / `rev-parse` 等只读命令）；所有变异实验、探针测试、判定器沙盒均位于 `/tmp/req002-qa-review/`；数据库口令全程未回显、未写入本报告或任何其他文件（本报告只出现变量名 `ONTOK_TEST_DATABASE_URL`）。

---

## 附：审核产物清单（均在 `/tmp/req002-qa-review/`）

| 文件 | 内容 |
| --- | --- |
| `baseline/` | 被审代码的纯净副本（变异实验的源） |
| `patch.py` / `mutate2.sh` | 变异注入器与运行器（每变异独立 target 目录） |
| `inject.py` / `gate_mut.sh` / `gate/` | 判定器可靠性实验（G1/G2/G3/G4/G7） |
| `ac012/` | AC-012 库级端到端复现探针 |
| `mutdiff/` | 各变异后的文件副本，可 diff 出每一处改动 |
| `REVIEW.md` | 本报告 |
