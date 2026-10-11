# 验收标准：REQ-084 develop 分支客户表名脱敏

> 依据：2026-10-11 负责人裁定「认真做 develop 侧脱敏（立 REQ 走 Dev+QA 闭环）」。
> 基线：本地 develop `4469e05`（已合并 REQ-081）。
>
> **⚠️ 本需求的第一交付物是清单，不是代码。清单没过审不得动源码。**

---

## 背景：为什么这件事比「find-replace」难（BA 实测，不凭记忆）

表名在 `develop` 分支里**不是文档提及，而是承重的输入**。这是它与 `demand` 文档的根本差别。

| 位置 | 处数 | 性质 |
| --- | --- | --- |
| `src/extract/propose.rs` | 145 | 大量是**词干匹配的算法论证**（94 处词法信号） |
| `src/domain/concept_groups.transition.json` | 138 | `include_str!` 进**生产代码**（`src/domain/concept.rs:727`），`confirmed_by: null` 的真实概念组声明 |
| `tests/` 全部 | **327** | 承重的测试夹具 |
| `tests/test_fk_naming.rs` | 14 | **红线文件** |
| `src/domain/view.rs` | 5 | **红线文件** |
| `fixtures/proposal-demo.json` | 3 | 与 `propose.rs` **契约绑定**（注释原文：「改这里要同步改那份 fixture」） |
| **合计** | **682** | 28 个文件 |

### 决定性反例：为什么不能一刀切

`src/extract/propose.rs:1235`（原文）：

```rust
/// `package_templates` 以 `template`（词干）结尾吗？**不以** ——
/// `template_id → package_templates` 根本命不中（实测 `stem`-only
```

这类测试**专门检验真实表名的拼写/词干/后缀属性**。换成 `t064` 之后，
「`t064` 是否以 `template` 结尾」这个问题**永远为假**，断言当场失去技术含义。

### 已完成的前置工作（不用重做）

| 分支 | 状态 |
| --- | --- |
| `demand` 文档 | **已完成** 240 处假名化，编号 `t001`…`t091` |
| `develop` 翻译表 | **已完成**，`src/extract/table_names.json` 脱敏为 `1.3.0-desensitized` |
| 对照表 | `.review-workspace/name-map-english.json`（本地，不入库） |

⇒ **develop 侧必须沿用同一套 `t001`…`t091` 编号**，否则两个分支对不上。

---

## AC-001 · 先出可脱/不可脱清单（**硬前置**）

**在改任何源码之前**，先交付一份逐条清单，字段如下（缺一不可）：

| 字段 | 要求 |
| --- | --- |
| 文件 | 相对路径 |
| 行号 | 精确到行 |
| 表名 | 原文 |
| 分类 | 只能是 `L` / `S` / `R` 三档之一 |
| 判据 | 这处**到底在测什么** |
| 理由 | 为什么属于该档 |

三档定义（**不得自创第四档**）：

- **`L` = 承重·不可脱**
  脱敏后，该处**被测的性质发生改变**，或断言变成恒真 / 恒假。
  必须写出机制说明：「脱敏后为什么失效」，不能只写「感觉重要」。

- **`S` = 安全·可脱**
  表名在该处**纯粹是不透明字符串**，替换后断言语义完全不变。
  必须指出「这个断言实际在测什么」，以证明它与名字的拼写无关。

- **`R` = 红线·不动**
  位于红线文件（`src/domain/view.rs` / `tests/drop_rate.rs` / `tests/test_fk_naming.rs`），
  且属于 `L`，或改动需要破坏红线纪律。

⚠️ **判定不得靠「看起来像夹具」**。`tests/test_overview.rs:306` 断言的是
`!hay.contains("package_templates")` —— 替换成 `t064` 后断言仍成立，
但它验的是「表名没有泄进总览」，**替换后这条判据的含义已经变了**。
这类必须逐条论证，不能批量归 `S`。

---

## AC-002 · 不可脱项集中登记并钉住

`L` 与 `R` 档的表名必须集中登记到**单一文件**，逐条写明理由与机制；
并加一道 `include_str!` 元测试钉住它（沿用 REQ-075 / REQ-081 的做法），
使「有人偷偷删掉登记」变成构建失败。

⚠️ **登记 ≠ 屏蔽**。不要因此把这些名字从代码里删掉——它们是承重的，
删掉就等于删测试。

---

## AC-003 · 判别力自证（必做，给二进制名 + 运行条数）

每类脱敏至少给一条**反事实**：挑一条你判为 `L` 的断言，**强行脱敏它**，
实测该测试是否变红。

- 变红 ⇒ 证明你的 `L` 分类是对的
- 仍绿 ⇒ **你的分类错了**，该条应改判 `S`，并说明为什么它不承重

**编译失败不算变异执行**（沿用本仓既有纪律）。每条给二进制名与运行条数。

---

## AC-004 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`，与基线同口径对比
- 红线三文件 `git diff --numstat` **无输出**；
  若你判断确需改红线，**先停下来说明，不要动手**
- **无测试被删除、无新增 `#[ignore]`、无断言被放宽**
- 存活变异必须逐条论证

---

## AC-005 · 产物 sha

纯脱敏**不应改变抽取产物**。

| 产物 | 登记 sha（改动前后都应不变） |
| --- | --- |
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` |
| `proposal.json` | 视脱敏范围而定；若因 `L` 档保留真名而**不变**，说明白；若变了，逐字段说明 |

⚠️ 若 `fixtures/proposal-demo.json` 被改动，`src/extract/propose.rs` 必须**同步改**——
两者是契约关系，单改一处会让 REQ-002 的对齐断裂。

---

## ⚠️ 本机环境的三个坑（不读会踩）

1. **`code/src/extract/table_names.json` 在本地是真版**（`git update-index --skip-worktree`），
   HEAD 里是脱敏版。**你的 worktree 拿到的是脱敏版** ⇒
   `drop_rate` / `test_fk_naming` 这两道门禁会因翻译表对不上而失败。
   **这是预期现象，不是你的 bug。** 跑门禁前先把真版拷进去：
   ```bash
   cp <仓库根>/.review-workspace/table_names.orig.json src/extract/table_names.json
   ```
   **且绝对不要 commit 这条命令改过的文件。**

2. **登记基线 `schema.json` 不入库**，门禁需要它：
   ```bash
   export ONTOK_SCHEMA_JSON="<仓库根>/.review-workspace/fix-props/schema.json"
   ```

3. **冷编译约 6 分钟**，各 worktree 自己编译。⛔ 禁 `CARGO_TARGET_DIR`、
   `--release`、另建 probe crate、`ln -s` 指向别的 target。

---

## 硬约束

| 项 | 说明 |
| --- | --- |
| 红线文件 | `tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs` **禁改** |
| 任何测试 | 不许删除、不许 `#[ignore]`、不许放宽断言 |
| `table_names.json` | 禁 commit（本地是真版，HEAD 是脱敏版） |
| 临时文件 | 只放 `.review-workspace/req084/`；**禁 `/tmp`、禁 home** |
| 删除文件 | 禁 `rm`（本环境已接管成 `mavis-trash`） |
| 仓库 | **无 remote，不要 push**；不 squash、不 force、不改历史 |
| BA 分支 | **只由 BA 写**——你那个分支上没有 `BA/` 目录，不要改；建议文案写在报告里 |
| 中文长文 | 一律用 edit/write 工具，**不要用 heredoc** |
| 量化数字 | **必须标口径与命令**；先剥注释再核对文件 |
| 口令 | 只走环境变量，不入库/不进文件/不进日志/不进 commit；**不要 echo 连接串** |

---

## 交付纪律

- **边做边把结果落盘**——本会话已有多次子 agent 会话丢失的先例
- 清单先落盘为 `BA/demands/REQ-084/table-name-inventory.md`（**报告里给内容，由 BA 落笔**）
- 遇到与本 AC 冲突的事实，**先说明再动手**，并给出你的判断与理由
