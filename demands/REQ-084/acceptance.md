# 验收标准：REQ-084 develop 分支客户表名脱敏（rev.4 · QA 独立验证后）

> **rev.3 依据 Dev 第二步实测推翻 rev.2 的两条裁定（AC-003 oracle 处理 / AC-004② 变异方向）。**
> rev.2 依据 **Dev 第一步清单**（695 行，`feature-REQ-084/.review-workspace/req084/inventory.md`）
> 与 **BA 逐条复核**写成。**rev.1 的 4 条事实性断言被推翻，全部按证据更正。**
> 基线：本地 develop `4469e05`。

---

## ⚠️ rev.1 → rev.2 更正（Dev 否定 BA，全部复核成立）

| # | rev.1 写的 | 实测 | 复核依据 |
| --- | --- | --- | --- |
| 1 | 基线 **682** 处 | **695** 处 | 口径差异：Python `re` 的 `\b` 在 **CJK 字符旁不成立**（`应` 被当作 `\w`），`关联模型ID(对应llm_model.model_id)` 这类**中文夹表名**被系统性漏掉 12 处；另 1 处在 `t_order`。差 13 集中 5 文件 |
| 2 | `proposal-demo.json` 与 `propose.rs` **契约绑定表名** | 契约是**顶层形状 + `round=1`，从不点名表名**；那 3 处表名位于 `evidence[].table`，是纯载荷 | `propose.rs:38/134/151` 原文 |
| 3 | `identifier_shape.rs:444` 大概率 S | `:444` 是 `customer_id`（**列名，不在 91 表名单**）；表名在 **`:445`（S）** 与 **`:446`（L）** | `sed -n '444,447p'` |
| 4 | transition.json 经 `include_str!` 进**生产代码** | 在 **`#[cfg(test)]` 内**（该块起于 `:520`，`include_str!` 在 `:727`）⇒ **纯测试夹具，零生产使用** | 同上 |

### 由此新增的工具层纪律

⛔ **禁止用 Python `re` 的 `\b` 统计中文语料里的标识符**。
必须用 `(?<![A-Za-z0-9_])…(?![A-Za-z0-9_])`，或先剥掉 CJK 相邻再统计。
本项目已因此错报 1 次（682 vs 695）。

⛔ 同族已踩坑记录（本会话共 4 次「朴素口径骗人」）：
`core.quotePath` 转义致中文文件名静默漏扫 / 扩展名白名单漏 `.py|.log|.bak` /
`xargs -0` 与 `-d` 同时指定 / 本条 `\b` 与 CJK。

---

## 第一步交付：三档分类（已完成，Dev 产出 BA 复核通过）

| 档 | 条数 | 含义 |
| --- | --- | --- |
| **`L`** 承重·不可脱 | **406** | 表名字面量是**被测性质本身**或其论证文本 |
| **`S`** 安全·可脱 | **269** | 表名是**载荷**，判据与拼写无关 |
| **`R`** 红线·不动 | **20** | `view.rs` 5 + `test_fk_naming.rs` 15；`drop_rate.rs` **0 处** |
| 合计 | **695** | 28 文件 |

Dev 自查时已发现并修正 6 处误判（`propose.rs` 测试模块内的 6 条**注释**原被归 `S`，
实际是词干机制的论证文本，已改判 `L`）。**这种自查我认可。**

---

## 负责人裁定（2026-10-11）：246 处的去留

`concept_groups.transition.json`（138）+ `tests/test_concept_transition.rs`（108）= **246 处（35%）**
按 **「仓库脱敏、本地同版」** 处理。

⚠️ **BA 补充裁定：不需要 skip-worktree 双版本。**
理由：该 json **只被三处测试读**（`concept.rs:727` 在 `cfg(test)` 内、
`test_concept_transition.rs:21`、`test_provenance_source.rs:23`），**零生产使用**。
它与 `table_names.json` 的关键差别：后者驱动真实抽取，前者只是测试夹具。
⇒ **仓库与本地都用脱敏版**，不存在两版打架，也不需要本地保留真版。

---

## AC-001 · 脱敏范围（按裁定重新划分）

| 桶 | 条数 | 处理 |
| --- | --- | --- |
| **① 脱敏** `concept_groups.transition.json` | 138 | 改 `t001`…`t091`，**保持结构不变**（91 个成员、概念分组、关系基数、`rule` 字段一律不动） |
| **① 脱敏** `tests/test_concept_transition.rs` | 108 | 与 ① 同批改。⚠️ **rev.1 判它 L 的前提已被裁定推翻**——声明脱敏后它必须跟随，否则 `home.get("t025")` 落空返回 `None` |
| **① 脱敏** 原 `S` 档 | 269 | 改假名，判据不动 |
| **② 登记保留** 其余 `L` 档 | 160 | **一字不改**，集中登记理由 |
| **② 登记保留** `R` 档 | 20 | 红线文件，**一字不改** |
| 合计 | **695** | |

⚠️ `tests/test_snapshot_write_paths.rs` 的 6 处 `order` 属 ② 保留档——
BA 已独立复核其机制：源码注释原文「两侧都是 `ORDER BY key`，而 `"customer" < "order" < "subscription"`」，
而 `t064`(0x74) > `subscription`(0x73) ⇒ 假名化后 `b.last()` 断言当场变红。
**这是纯字面量序依赖，与词干逻辑无关。**「看起来像纯夹具 ⇒ 归 S」的批量判法必然漏掉它。

---

## AC-002 · 保留项集中登记并钉住

160 条 `L` + 20 条 `R` 共 **180 处**，逐条写明「为什么脱敏会让被测性质消失」，
登记到**单一文件**，加 `include_str!` 元测试钉住（沿用 REQ-075 / REQ-081 做法）。

⚠️ **登记 ≠ 屏蔽**。不得因此把这些名字从代码里删掉——它们承重，删了等于删测试。

⚠️ `R` 档建议拆两类登记（Dev 异议 4，已采纳）：
`R-代码`（`view.rs:87` 的 `BUSINESS_WORDS` 词根条目）/ `R-注释`（其余 4 处）。

---

## AC-003 · 泄漏 oracle：**两侧同批替换**（rev.3 更正，推翻 rev.2 写法）

`S` 档里有一类**判据串**（泄漏 oracle），例如 `tests/test_overview.rs:296/298/306`：

```rust
:296  g("package", "套餐", &["package_templates", "package_products"], …)
:298  Evidence { table: "package_templates", column: "template_no", … }
:306  assert!(!hay.contains("package_templates"), "表名泄进了总览")
```

⚠️ **rev.2 曾写「oracle 侧保留真名、只脱敏输入侧」—— 该写法已被推翻。**
输入换成 `t067` 后渲染路径不再吐真名，`!hay.contains("package_templates")` **恒真**：
泄漏守卫**静默失效且不报错**。那比留着真名更危险，因为它看起来是绿的。

⇒ **正确处理：输入侧与判据侧同批替换**，断言写成 `!hay.contains("t067")`，
守卫的牙保留（渲染路径若打印该串仍会红），并在登记文件里注明「判据串为假名」。

⚠️ **D 组例外**（`ontok-gate-preflight.rs`，18 处）**必须成组、且已裁定全留**：
同一条 `#[test]` 内另有 12 处夹具与判据耦合，只换一侧 ⇒ `find().unwrap()` 直接 panic。
Dev 已按此改判为 `L-改判`。

Dev 识出的 oracle 共 **15 处 / 4 组**：

| 组 | 位置 | rev.3 裁定 |
| --- | --- | --- |
| A | `test_overview.rs:296/298/306` | **两侧同批替换** |
| B | `html_view.rs:1035/1044` | **两侧同批替换** |
| C | `test_mcp_server.rs:768/770/776` | **两侧同批替换** |
| D | `ontok-gate-preflight.rs:2031/2044-2047/2062`（含耦合夹具共 18 处） | **全留**（已改判 `L-改判`）

---

## AC-004 · 判别力自证（必做，给二进制名 + 运行条数）

⚠️ **rev.3 更正变异方向。rev.2 写的是「还原」变异，那个推论是错的。**

rev.2 写「把 ① 脱敏批次整体还原 ⇒ 至少一条测试变红；全绿 ⇒ 没有人盯着」。
**Dev 否定成立**：① 批次的定义就是「判据与拼写无关的载荷」——**两种拼法都自洽本来就该都绿**。
拿「还原后全绿」反推「没人盯着」是范畴错误。

⇒ **正确的判别力变异是「分叉」**：只回退耦合的一侧，让耦合断裂。

1. **分叉变异（主）**：只回退 `concept_groups.transition.json`、保留测试侧假名
   ⇒ 预期 `test_concept_transition` 变红（Dev 实测 6 passed / 4 failed）
2. **承重变异**：抽 1 条 ② 保留档的 `L` 断言，强行假名化 ⇒ 实测变红
   （Dev 实测 `test_snapshot_write_paths` 8 条中 1 条 FAILED，且失败点正是
   BA 预判的 `:269` `b.last()` —— `t064` 0x74 > `subscription` 0x73）
3. ⛔ **编译失败不算变异执行**；回滚后若用 `copy2` 之类保留 mtime，
   **必须 `touch` 强制重编**——否则 cargo 判定 `include_str!` 输入未变而跑陈旧二进制，
   得出假红假绿。（Dev 本次已踩，需写入 `dispatch/rules.md`）

---

## AC-005 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`，与基线同口径
- 红线三文件 `git diff --numstat` **无输出**（`view.rs` 5 + `test_fk_naming.rs` 15 均判 `R`）
- **无测试删除、无新增 `#[ignore]`、无断言放宽**
- `concept_groups.transition.json` 脱敏后：**26 组 / 67 成员**、概念分组、`rule` 字段**必须逐字保留**

---

## AC-006 · 产物 sha

纯脱敏不应改变抽取产物。

| 产物 | 登记 sha |
| --- | --- |
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` |
| `proposal.json` | 若变，逐字段说明 |

⚠️ `fixtures/proposal-demo.json`（`S` 档 3 处）可独立改，**`propose.rs` 无需同步**
——rev.1 的「契约绑定」说法已按证据撤销（见更正表第 2 条）。

---

## ⚠️ 环境坑

本机 `code/src/extract/table_names.json` 是**真版**（`skip-worktree`），HEAD 是脱敏版，
**你的 worktree 拿到脱敏版** ⇒ `drop_rate` / `test_fk_naming` 会失败。
**这是预期，不是 bug。** 跑门禁前：

```bash
cp <仓库根>/.review-workspace/table_names.orig.json src/extract/table_names.json
```

⛔ **不要 commit 这次覆盖。**

门禁还需登记基线（不入库）：
```bash
export ONTOK_SCHEMA_JSON="<仓库根>/.review-workspace/fix-props/schema.json"
```

冷编译约 6 分钟。⛔ 禁 `CARGO_TARGET_DIR` / `--release` / 另建 probe crate / `ln -s`。

---

## 硬约束

| 项 | 说明 |
| --- | --- |
| 红线文件 | `tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs` **禁改** |
| 任何测试 | 不许删除、不许 `#[ignore]`、不许放宽断言 |
| `table_names.json` | 禁 commit（本地真版 / HEAD 脱敏版） |
| 临时文件 | 只放 `.review-workspace/req084/`；**禁 `/tmp`、禁 home** |
| 删除文件 | 禁 `rm`（本环境已接管成 `mavis-trash`） |
| 仓库 | **无 remote，不要 push**；不 squash、不 force、不改历史 |
| BA 分支 | **只由 BA 写**——建议文案写在报告里 |
| 中文长文 | 用 edit/write 工具，**不要用 heredoc** |
| 量化数字 | **必须标口径与命令**；⛔ 禁 `\b` 统计中文语料（见新增纪律） |
| 口令 | 只走环境变量，不入库/不进文件/不进日志/不进 commit；**不 echo 连接串** |

---

### ⚠️ 本 AC 曾给出一条**假绿配方**的命令（QA 判 N2，已修正）

rev.1~rev.3 里我写的扫描命令是：

```
git ls-files -z -c core.quotePath=false     # ❌ 错
```

在本机 **git 2.56.0** 上 `-c` 放在子命令之后**不被当作全局配置**，
该命令**静默返回 0 个文件** ⇒ 任何基于它的扫描都会报「零泄漏」。

✅ 正确写法（`-c` 必须在子命令**之前**）：

```
git -c core.quotePath=false ls-files -z     # 实测 117 个文件
```

⚠️ 这条错命令被我写进了 AC **和派单 briefing**，Dev 与 QA 都照抄过。
**QA 是因为结果可疑地等于 0 才抓到的。** BA 的错误配方会一路传染给下游 agent。

### ⚠️ rev.3 的「91 个成员」是错的（QA 推翻）

`concept_groups.transition.json` 的实际结构是 **26 个概念组 / 67 个成员**。
「91」是 `tests/test_concept_transition.rs` 里 `SOURCE_TABLES` 的表清单长度，
不是声明的成员数。BA 两处混写了。Dev 的 history note 写的是对的。

## 交付纪律

- **边做边落盘**——本会话已有多次子 agent 会话丢失先例
- 你不同意本 AC 时，**先说明再动手**，给证据
