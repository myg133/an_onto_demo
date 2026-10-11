# 三、红线确认 / 四、阻断项 / 五、纪律自证

## 红线（四项全部未动）
`git diff 90d9046..504ae9f` 逐项：
| 红线 | 结果 |
|---|---|
| `tests/drop_rate.rs` | UNMODIFIED ✓ |
| `tests/test_fk_naming.rs` | UNMODIFIED ✓ |
| `src/domain/view.rs` | UNMODIFIED ✓ |
| `.review-workspace/fix-props/schema.json` | 存活 ✓ 219688 字节，mtime 10月7日 23:55（早于本次审查），sha256 `b71c2afde6c103132e4e1ca6b9896aab442bd80abbe1fcc0870f35ddce0f2019` |

## 阻断项清单
**无阻断项。**

## 非阻断发现（按严重度）
### F1（中）· `ToolProduced` 无写入方 ⇒ `source` 当前零判别力
生产代码零处写 `ToolProduced`（穷举证据见 `04-source-semantics.md` §4）；
库里两表 `tool_produced` 计数 = 0。后果：ADR-003 (b) 字面要求的
「区分『工具自产』与『外部声明』」只落地一半，`is_declared()` 对 import 写入的行恒真。
**方向保守（无误标为可信），故不阻断**；Dev 已主动披露。后续项建议记 REQ-023 / backlog。

### F2（低）· 新增的 `DEFAULT_PROPOSAL_*` 常量在规则路径上没被用
`src/extract/propose.rs:163` 定义 `DEFAULT_PROPOSAL_BASE_VERSION`、
`:157` 定义 `DEFAULT_PROPOSAL_ROUND`，理由写的是
「两处各写一个 `1` 看着一样，改的时候只改一处就分叉了」。
但 `propose.rs:457-458` 仍是**字面量** `base_version: 1` / `round: 1`；
常量只在 `agent/assemble.rs:179-184`（重建路径）被用。
⇒ **规则路径恰恰是常量没覆盖的那一半**，注释宣称要消除的漂移风险原样留着。
今天无行为差异（都是 1），故不阻断；属「重构做了一半」。

### F3（低·生产不可达）· 重建路径把 `None` 与 `Some("")` 又合回去了
`entity_display_name` 的文档（`propose.rs:226-228`）明确写：
「`None` 与 `Some("")` **刻意不同**…… 合成一个返回值就抹掉了这个区别」。
但全量路径 `propose.rs:368-377` 能区分（`Some("")` 不进 `tables_without_cn_name`），
而重建路径 `assemble.rs:152-156` 用 `display_name.is_empty()` 判断 ——
**无法区分二者**，`Some("")` 会被误记进 `tables_without_cn_name`。
可达性：生产构造 `TableNames` 的**唯一**入口是 `ontok-propose.rs:124` 的
`TableNames::load`，而 `names.rs:49-51` 对空/纯空白中文名 **fail-closed 直接 bail**，
故 `lookup()` 生产上返回不了 `Some("")`。
⇒ **今天生产不可达**，仅在未来外置 agent 或绕过 `load()` 构造时才会暴露。记录备查。

### F4（低）· import 通道唯一一处行为放宽
`cli.rs:466-482` 额外接受 `failed_groups` 的对象形状（取 `group_id`）。
改动前报错、改动后静默接受。方向合理且仍标 `declared`，但 ADR-003 恰点名
`failed_groups` 是「真正危险的字段」，建议验收记录留一笔（见 `04` §5）。

## 纪律自证
- `feature-REQ-022` worktree：**`git status --porcelain` 空**（0 行输出），HEAD = `504ae9f`。
  未创建、未修改、未删除该 worktree 内任何文件。
  （复核说明：该 worktree 下部分文件 mtime 在近 40 分钟内变动，是**另一 QA 的变异循环**
  在动；我的关键读数一律用 `git show 504ae9f:` / `git grep 504ae9f` 从**不可变 commit 对象**
  取，故不受影响。）
- 未执行 `CARGO_TARGET_DIR` / `--release` / `cargo build` / 新建 probe crate。
  只运行了既有的 `target/debug/ontok` 与既有的预编译测试二进制。
- 我创建的全部临时文件都在 `.review-workspace/qa-req022b/`：
  `01-propose-diff.md`、`02-from-relation-verbatim.md`、`03-shared-rules-evidence.md`、
  `04-source-semantics.md`、`05-redlines-and-findings.md`、`propose.diff`、
  `base_propose.rs`、`head_propose.rs`、`base_from_relation.txt`、`head_from_relation.txt`、
  `base_links.txt`、`head_links.txt`、`forged_proposal.json`、`last_project.txt`、
  `pgq.py`（手写 PG 线协议客户端，未连通，保留作过程记录）、`.venv/`（psycopg2，只读查询用）。
- 数据库：只 `INSERT`（1 个项目 + 1 个版本 + 1 个提案 + 1 条 apply 产出 + 3 条 Dev 测试已写入）。
  **零 DELETE / TRUNCATE / 清表**。全部查询 `set_session(readonly=True)`。
- 口令：只经环境变量，未写入任何文件/日志/提交信息。
