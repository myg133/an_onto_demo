# 验收标准：REQ-087 用 `is_primary_key` 产出 `1:1`（rev.2）

> **rev.2 记录 2026-10-12 负责人裁定（折中方案）：规则写好入库，但【不接进 `infer_link`】。**
>
> 两条裁定合并理解：
> 1. **产品定位 = 可复用的多库场景** ⇒ `1:1` 在别的客户库（共享主键关联表很常见）是**真需求**，
>    规则不该删。
> 2. **但 `semantic_cn` 不认识 `corresponds_to`** ⇒ 一旦接线并命中，
>    语义原样透出 → 第四档 `untranslated_semantic_hit` 抓住 → **整份客户视图拒绝发布**。
>
> ⇒ **收益留、风险封。接线的唯一前置是 `semantic_cn` 能翻译该语义**
> （已另立 **REQ-089**，需红线 `view.rs` 特批一行）。
>
> 立项依据：2026-10-11 explore 摸底（80 条关系实际形态）+ BA 复核
> 基线：本地 develop `35e4a09`

---

## rev.2 的具体要求

| 项 | 要求 |
| --- | --- |
| `infer_link` 行为 | **与基线 `35e4a09` 逐字节一致** —— 本次不接线 |
| `cardinality.rs` | 保留：规则本体、`evidence_claim`、已知限制登记、单元测试 |
| `mod.rs` | 保留 `pub mod cardinality;` |
| 单元测试 | ⚠️ **当前测试走 `links_for_table` 端到端；不接线后它们会红**。必须改成**直接测 `card_for`**，且**不得拓宽任何可见性** |
| 模块头注释 | 写明「**本规则尚未接线**；接线前置 = `semantic_cn` 能翻译 `corresponds_to`（REQ-089）」 |
| 全量 | 失败必须为 **0**；`passed` 允许因新增单测而高于基线 |
| `propose.rs` | 仍是 **12 增 12 删零行号平移**（REQ-084 登记表钉死了行号+列号） |

---

## 背景：先说清楚这个需求**不**做什么

explore 在摸底 80 条关系时发现，本轮最硬的一条问题是：

> `cardinality` 没有任何数据依据 —— 它是「列名以 `_id` 结尾」的**同义反复**。

现状（`src/extract/propose.rs:224-230`）：

```rust
REL_SUFFIXES: [("_ids", "1:N", "has"), ("_id", "N:1", "belongs_to"), …]
```

`links_for_table`（`:538-575`）逐列调 `infer_link(c, &t.name, known_tables)`，
**只看列名后缀**，不看 `c.is_primary_key`。

⇒ **80 条全 `N:1` 是命名惯例，不是观测结论。**

### ⛔ 本需求明确**不**宣称解决这件事

判定 `N:1` 与 `1:1` 的分野，需要「该列的值在表内唯一」这个事实。
拿不到的原因有二，且**都不是本需求能修的**：

| 缺失 | 原因 |
| --- | --- |
| UNIQUE 约束 | `ColumnMeta`（`src/extract/introspect.rs:41-48`）只采 `data_type`/`nullable`/`comment`/`is_primary_key`，**全文无 `unique`** |
| 值域重叠 | 源库只读 `information_schema`/`pg_catalog`，**零业务数据行** |

⇒ 登记为**已知限制**。本需求做的是：**把一个已经采到、已经发给 LLM、却在规则路径上被忽略的判据接回去**，
让 `1:1` 从「原理上不可产出」变成「在可判定的那一小类里可产出」。

---

## 实况：这条规则在当前语料下**零触发**（必须如实登记）

| 事实 | 数字 | 口径 |
| --- | --- | --- |
| 登记基线里标为主键的列 | **93** | `schema.json` 中 `is_primary_key == true` |
| 80 条关系的映射列 | 0 条是主键 | 按 `(from_type, evidence[0].column)` 比对 |
| 本需求对当前产物的改变 | **零** | 80 条基数全不变 |

⚠️ **不得把本需求写成「解决了基数无依据」**。同 REQ-066 残留 S6 的处置口径：
按指定口径登记，**不写成「已封堵」**。

---

## AC-001 · `is_primary_key` → `1:1`

`infer_link` 或其调用处读取 `c.is_primary_key`：
命中该列且该列**是本表主键**时 ⇒ 基数判 `1:1`、语义 `corresponds_to`（唯一对应）。

**先查清既有词表**：若 `唯一对应` 已是既有词形则复用，不要新造第四个词。

**方向约定不变**：仍是「子表 → 主表」。`customer_subscriptions —1:1→ customers`
若真成立，意思是「一条订阅恰好对应一个客户」——这正是该表自己的主键语义。

---

## AC-002 · 判别力自证（**本需求的全部价值所在**）

> ⚠️ **rev.2 批注（QA 指出本条与 rev.2 文档层面自相矛盾）：**
> 第 3 步写「若第 1 步给不出 `1:1` ⇒ 不得声称完成」，但 rev.2 已把 `infer_link` 钉成**不接线**。
> ⇒ 照字面验**已交付代码**会**误判 FAIL**，而代码正确。
> **本条以「变异验证」为准**：把 `WIRED` 改成 `true` 后，构造样本必须产出 `1:1`；
> 非接线状态下不要求、也不应该产出 `1:1`。

因为真实语料零触发，**必须用构造样本自证**，否则等于没做：

1. 造一张表，`customer_id` 既是普通列、**又**是本表主键，且该列以 `_id` 结尾
   ⇒ 实测基数产出 `1:1`（不是 `N:1`）
2. 把 `is_primary_key` 改回 `false` ⇒ 实测退化为 `N:1`
3. **若第 1 步给不出 `1:1`** ⇒ 判据没接上，不得声称完成

⚠️ **编译失败不算变异执行**。每条给**二进制名 + 运行条数**。
⚠️ 构造样本放 `.review-workspace/req087/`，**⛔ 不要把构造样本写进 `tests/` 夹具**——
那会让「零触发」变成「有触发」，污染真实语料。

---

## AC-003 · 与既有判据的关系

| 项 | 要求 |
| --- | --- |
| `html_view::count_phrase`（`:115`） | 已是全项目唯一的数量词出口。**本需求只改基数产出，不改它**。新增 `1:1` 时该函数必须已认识对应词形，否则**两边会说不同的话** |
| `overview.rs:307` | 复用同一函数，**不得另写一份** |
| 已知限制 | 在代码注释里写明「`N:1` 仍是列名后缀推断，非观测结论；判定需要 UNIQUE 约束或值域，当前拿不到」 |

⚠️ 若 `count_phrase` 遇到不认识的词形会落到兜底分支 —— **那会让页面说「多个」而实际是 1 个**。
新增词形必须让 `count_phrase` 认识它。

---

## AC-004 · 已知限制登记（与代码同处）

在 `infer_link` 的文档注释里写清三件事：
1. `N:1` 来自列名后缀，**不是观测**
2. 判 `N:1` vs `1:1` 需要 UNIQUE 约束或值域，**当前架构拿不到**
3. 源库零外键 ⇒ 目标表命中靠列名/词干匹配，**也可能命中错**

---

## AC-005 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`，与基线同口径（基线 **773 passed / 0 failed / 1 ignored / 59 组**）
- **当前产物必须零变化** —— 80 条关系的基数、语义、方向**逐条不变**。
  若有变化，说明规则误触发，**是 bug 不是效果**
- 红线三文件 `git diff --numstat` **无输出**
- 无测试删除、无新增 `#[ignore]`、无断言放宽
- `stats.json` / `proposal.json` 的 **sha 不得变**

---

## ⚠️ 环境坑

本机 `code/src/extract/table_names.json` 是**真版**（`skip-worktree`），
**你的 worktree 拿到的是脱敏版** ⇒ `drop_rate` / `test_fk_naming` 会失败。
**这是预期，不是 bug。** 跑门禁前：

**坑 0（QA 实测踩过，失败特征最容易误判）：worktree 的 `table_names.json` 是**脱敏版** `t001…`，真版要手动拷。**

漏拷时**不会测试红**，而是**产物静默降级**：中文名覆盖掉，
`entities_with_cn_name` 掉到 3、`props_via_relation` 61→0。
极易被误读成「规则没接线」而错判 FAIL。

```bash
cp <仓库根>/.review-workspace/table_names.orig.json src/extract/table_names.json
export ONTOK_SCHEMA_JSON="<仓库根>/.review-workspace/fix-props/schema.json"
```

⛔ **不要 commit 这次覆盖。** 交回前 `git checkout HEAD -- src/extract/table_names.json`。

冷编译约 6 分钟。⛔ 禁 `CARGO_TARGET_DIR` / `--release` / 另建 probe crate / `ln -s`。

---

## 硬约束

| 项 | 说明 |
| --- | --- |
| 红线文件 | `tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs` **禁改** |
| 任何测试 | 不许删除、不许 `#[ignore]`、不许放宽断言 |
| 构造样本 | 只放 `.review-workspace/req087/`，⛔ **不许进 `tests/` 夹具** |
| `table_names.json` | 禁 commit |
| 临时文件 | 只放 `.review-workspace/req087/`；**禁 `/tmp`、禁 home** |
| 删除文件 | 禁 `rm`（本环境接管成 `mavis-trash`） |
| 仓库 | **无 remote，不要 push**；不 squash、不 force、不改历史 |
| BA 分支 | 只由 BA 写 |
| 中文长文 | 用 edit/write 工具，**不要 heredoc** |
| 量化数字 | **必须标口径与命令**；⛔ 禁 `\b` 统计中文语料；⛔ `git ls-files` 扫中文路径须 `-z` + `-c core.quotePath=false` 且 `-c` 在**子命令之前** |
| 口令 | 只走环境变量，不入库/不进文件/不进日志/不进 commit；**不 echo 连接串** |

---

## 交付

1. 改动摘要（文件:行号 + 改了什么）
2. AC-002 三条自证结果（构造样本 → 二进制名 + 运行条数 + 实测输出）
3. **「当前产物零变化」的证据**（80 条关系逐条比对 + 两个产物文件的 sha）
4. 已知限制登记的代码位置与原文
5. 全量结果
6. 红线 `git diff --numstat` 原始输出
7. 对本 AC 的异议（无则写「无」）

## 你可以否定 BA

本项目 Dev 否定 BA 被采纳 18 次。特别是：
- 若「零触发」意味着这条规则**不该做**（规则不可达即死代码）⇒ 说明理由并给证据
- 若 `count_phrase` 缺少对应词形而补它属于**扩 scope** ⇒ 说明，由 BA 决定要不要拆
