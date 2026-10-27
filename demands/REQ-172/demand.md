# 需求：REQ-172 · 表间关系探索（规则候选 → 值域验证 → AI 两轮 → 人签）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-172 |
| 优先级 | **P1**（决定"库驱动打底"能不能产出**可查、连得上**的本体） |
| 状态 | 已就绪 |
| 基线 | `develop` `a278aec` 之上（REQ-171 合并后） |
| 触发 | 用户 2026-10-27 23:47：「本体和表映射上了，但映射的不是全部表，那 R2RML 查询是不是有的数据查不到？按这个逻辑，需要 AI 去探索表之间的关联关系吧」；**用户拍板方案 A**（规则 → 值域验证 → AI 两轮 → 代码校验 → 人签） |
| 配套契约 | `Deploy/skills/ontok-relate/SKILL.md`（**BA 写**，判定规范） |

## 问题（真 UAT 库实测）

| 项 | 值 |
| --- | --- |
| 表 | **96** |
| **外键约束** | **0 条** ← 数据库没声明任何关联 |
| 有单列主键 | 95 / 96 |
| `*_id` 命名能落到某张表的**候选关系** | **51 条**（如 `balances.customer_id → customers.id`） |
| 参与候选的表 | 47 / 96 |
| **完全孤立**（连 `*_id` 线索都没有） | **49 张** |

⇒ 现在 `build run`（文档链）产出的本体只覆盖 **7 / 96** 张表 ⇒ **R2RML 只出 7 个 TriplesMap，其余 89 张查不到**。
⇒ 表间关系为 **0**（无外键、`build run` 也不调 `propose`）。

## 已存在但没接上的能力

| 组件 | 做什么 | 位置 |
| --- | --- | --- |
| `extract::fk::candidates` | `*_id` 后缀 → 目标表 PK 候选 | `src/extract/fk.rs:305` |
| `propose` | 表→类、列→属性、`*_id`→关系（**零外键下靠命名约定**，注释明写「`N:1` 是命名约定的复述，**不是观测结论**」） | `src/extract/propose.rs:14/264` |
| `ai_semantic` | **AI 判定**"这条候选关系在业务上说不说得通" | `src/extract/ai_semantic.rs`（已接在 `ontok-propose`） |
| `doc_merge` | 消费 `maps_to`（关系→两列） | `src/extract/doc_merge.rs` |

⇒ 本单把它们**串成一条链**，并补上**值域验证**与**补漏轮**。

## 算法（方案 A，五步）

### Step 1 · 候选生成（**确定性，不花 token**）

| 信号 | 做法 | 强度 | 现状 |
| --- | --- | --- | --- |
| 命名 | `{t}.{x}_id` → 目标表 `x`/`xs` 的 PK | 中 | 已有（51 条） |
| **值域验证** ⭐ | 对 `*_id` 列抽样（`SELECT DISTINCT col LIMIT 50`，**只读**），值集合 ⊆ 目标表 PK 值集合 ⇒ 升级「值域对上」 | **强** | 新增 |
| 同名列 / 共享枚举 | 两表同名列、小基数枚举 | 弱 | 新增 |
| 注释语义 | 列注释含"XX 编号/ID"且与目标表中文名匹配 | 弱-中 | 新增 |

产出 `candidates.jsonl`：`(child_table, child_col, parent_table, parent_col, evidence[], score)`。
⚠️ **值域验证必须只读**（`default_transaction_read_only=on`），且**抽样不落盘**（只用于判定，不进产物）。

### Step 2 · 证据打包（代码）

每张表一张"**名片**"：表名 + 中文注释 + PK + 列（名/类型/注释/非空）+ **2~3 行样本**（**脱敏**：数字/长串截断，不落 prompt 之外）。
复用 REQ-170/171 的 schema digest 机制，扩成**关系视角**（"这一对的两端各是什么"）。

### Step 3 · AI 两轮（**判定规范在 skill，代码负责投喂**）

| 轮 | 输入 | 问题 | 产出 |
| --- | --- | --- | --- |
| **A · 判定** | 候选 + 两端名片 + 样本 | 成立 / 不成立 / 证据不足 | 谓词（16 主键）+ 依据字段 |
| **B · 补漏** | 一张表的名片 + **其余表的一句话索引** | 哪些表与它有关系？ | 新增候选（`source: ai`，**低置信**，每表 ≤3 条） |

**Round B 是那 49 张孤立表的唯一入口。**

### Step 4 · 校验（代码，**质量兜底**）

| 校验 | 不通过 |
| --- | --- |
| 谓词在 16 主键里 | 丢 |
| join 列存在、类型可比较 | 丢 |
| 目标列是 PK/唯一 | 丢或降级「弱关系」 |
| 与已签关系基数矛盾 | 标 `conflict`（⛔ 不先到者赢） |
| 反向/同义重复 | 归一 |

每条带 `source: naming|domain|ai` · `confidence` · `band` · `reviewed_by: null`。

### Step 5 · 人签（**不变**）

页面按 `source` 分组（"数据库命名 51 / 值域验证 X / AI 推断 Y"），逐条签；**只有签过的进 R2RML**。

## 要做四件

### R1 · `introspect`/新命令：值域验证（纯代码）

`ontok-builder relation probe --schema <schema.json> --dsn <url> --out candidates.json`（名字可定）
- 只读、抽样、可配（`--sample-rows`、`--max-pairs`）
- 输出每条候选的 `evidence[]`（`naming` / `domain` / `samelenum` / `comment`）

### R2 · AI 两轮（代码 + 契约）

- 读 `Deploy/skills/ontok-relate/SKILL.md` 作为判定契约（⛔ 路径不写死，`--relate-skill` 或默认约定）
- Round A / Round B 各一个 prompt + 严格 JSON 协议（复用 `doc_acp` 那套：协议进 `prompt_sha`、缓存、失败要响）
- ⛔ 不许静默回落"只做规则"（失败要报）

### R3 · 落提案

- 候选 → 提案的 `link_types`（`reviewed_by: null`、`band: low`、`payload.evidence` 带来源）
- 与 `doc merge` 的关系断言**同一形状**（别造第二套）

### R4 · `build run` 串起来

`build run --dsn` 时：`introspect` → **propose 打底**（96 张全覆盖）→ 关系探索（R1+R2）→ `doc merge --proposal`（补语义）→ 草稿 → 评审台
（`--with-propose` 可显式开关；默认行为要写进 help）

## 判据（必须能红）

| # | 判据 |
| --- | --- |
| 1 | 值域验证：构造"值域 ⊆ PK"与"值域 ⊄ PK"两组真库夹具 ⇒ 前者 `evidence` 含 `domain`，后者**不**含 |
| 2 | 抽样**只读**：连接串强制 `default_transaction_read_only=on`；写入必被拒 |
| 3 | Round A：给一条"成立"与一条"不成立"的候选 ⇒ 前者出关系、后者不出；且**没有依据字段**的必须被丢 |
| 4 | Round B：孤立表（无 `*_id`）能被提出来，且 `source: ai`、`band: low`、每表 ≤3 条 |
| 5 | 校验层：谓词不在 16 主键 / 列不存在 / 目标非 PK ⇒ 各自丢弃并计数 |
| 6 | 端到端（真 UAT 库，只读）：`build run --dsn` ⇒ 提案里实体 **≈96**、关系 ≥ 已签候选数；R2RML 的 TriplesMap **≥ 90**（不再是 7） |
| 7 | 零回归：无 `--dsn` 的纯文档路径产物**逐字节不变** |

## 范围

✅ `src/extract/**`、`src/cli.rs`、`src/review_open/build_run.rs`、`src/domain/**`（如需）、测试；新增 `Deploy/skills/ontok-relate/**`（BA 写）。
⛔ `src/query/**`、`src/server/**`、`src/mcp/**`、`src/studio/**`、`src/db/**`、`src/provision/**`、`src/console/**`。

## 成本预算（96 表）

| 轮 | 调用 | 约 token |
| --- | --- | --- |
| A 判定（51 候选，10/批） | 6 | ~40k |
| B 补漏（96 表，12/批） | 8 | ~60k |
| 合计 | 14 | **~100k** |
