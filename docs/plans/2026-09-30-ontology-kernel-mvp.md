# 本体内核 · 最小可运行 MVP（Rust）

> **本文是范围规划，不是实施计划。** 范围确认后再展开成逐步实施文档。
>
> 上游：[`ontology-core-kernel.md`（Python 3600 行，2026-09-24）](2026-09-24-ontology-core-kernel.md) · [设计规格](../design/ontology-platform-spec.md) §0.2
> 状态：**范围已确认（2026-09-30）· 待展开为逐步实施文档**

---

## 0. 为什么重写

| 旧计划的语言 | 决定 | 依据 |
| --- | --- | --- |
| Python 3.12 / SQLAlchemy / Pydantic | **Rust 1.86 / sqlx / serde** | 采集器「一条命令部署」需要单静态二进制（规格 §4.2）；强类型守住存储契约 |

> **旧计划的领域逻辑定义（快照契约 / 双轨状态机 / diff 分类）语言无关，本计划沿用其语义，只换实现语言。**
> 「必须留位」的结论也全部继承 —— 那是架构决定，不是语言决定。

---

## 1. MVP 判据

> **一个 CLI，能把一份提案从提交走到冻结，并且两个版本之间能 diff。**

跑通这串命令即达标：

```bash
ontok project new --name acme
ontok version new --project acme                       # v1（草稿）
ontok propose import --file fixtures/proposal-demo.json
ontok review decide --proposal p1 --item 1 --track biz   --accept
ontok review decide --proposal p1 --item 1 --track model --accept
ontok version freeze --version 1
ontok ontology show  --version 1                        # 客户视图，零建模术语
ontok ontology diff  v1 v2
```

**判 MVP 达标的三个问题**：
1. 提案能走完双轨审核并冻结吗？
2. 冻结后能不能 diff 出「这一轮改了什么」？
3. 客户视图读起来像不像人话（没有主键/基数/外键）？

---

## 2. 范围 · 做什么

### 2.1 容器层

| 项 | 内容 |
| --- | --- |
| Project | 隔离单位（规格 §10.2） |
| OntologyVersion | 快照的归属，带 `seq` 与 `frozen_at` |

### 2.2 本体 schema（切片 1 落地部分）

| 元素 | 字段要点 |
| --- | --- |
| **ObjectType** | `key` · `display_name` · `description` · `aliases[]` · `parent`(层级) · `properties[]` · `evidence[]` |
| **Property** | `name` · `data_type` · `nullable` · `source_column` |
| **LinkType** | `key` · `from_type` · `to_type` · `cardinality` · `semantic` · `evidence[]` |

### 2.3 治理主干

| 项 | 说明 |
| --- | --- |
| **提案制** | 抽取产出的是 Proposal，不是直接改本体（规格 §2.2） |
| **双轨审核状态机** | 业务轨（客户）+ 建模轨（FDE）独立判定；关系类条目依赖两端已被接受 |
| **客户视图投影** | 序列化结果**零建模术语**（有测试守住） |
| **提案应用 + 版本冻结** | 链式 apply 累积而不重置；冻结后拒绝一切写入 |

### 2.4 必须占位（不实现逻辑，但必须在快照契约里）

> 这是**红线**：漏掉任一项，对应切片必然推翻本 MVP 的数据结构。

| 占位项 | 归属 | 依据 |
| --- | --- | --- |
| `ActionType` / `Function` | 快照 + 表 | 规格 §4.6，切片 2 |
| `WriteBinding`（`value_map` / `supported_values` / `conflict_field` / `key_expr`） | 快照（嵌在 `ActionType.effects`） | 规格 §5.2，切片 2 |
| `extracted_by`（agent 标识 + **具体版本** + 轮次 + 失败组） | 快照根 | 规格 §8.4，**旧计划完全缺失** |
| `disjoint_with[]` | 字段占位，**不做逻辑** | 规格 §4.6，导出时才用 |
| 抽取失败组 `failed_groups[]` | 快照根 | 规格 §7.1⑤，**不参与收敛判定** |

---

## 3. 范围 · MVP 暂缓（**不是永久不做**）

> ### ⚠ 本节与设计规格 §11「明确不做」是**两回事**
>
> | | 设计规格 §11 | 本节 |
> | --- | --- | --- |
> | 语义 | **永久不做**，有明确取舍理由 | **推迟**，后面要做 |
> | 处理 | 「不要因为顺手而加回来」 | 「现在没排上，不是不要」 |
> | 例 | 图数据库强依赖、项目内行级权限 | Store trait 抽象、HTTP API |
>
> **本节每一条都标注了「何时补回」。** 看到本表被当成「永久砍掉」时，先查这一列。

| MVP 暂缓 | 为什么现在不做 | **何时补回** |
| --- | --- | --- |
| **Store trait 抽象** | MVP 只有一个 PG 实现。**契约是 `Snapshot` 这个类型，不是 trait**（规格 §4.6 说的「唯一数据契约」指的就是它） | 第二种存储真正出现时 —— GraphDB 上量（§4.4）评估通过后 |
| **内存实现** | 同上 | 同上，契约测试要两套实现时 |
| **建模视图投影** | 判据是**客户能不能看懂**（AC-005）。建模视图是 FDE 内部工具，现在没有真实审核场景 | 切片 1 第 3 份计划（审核界面与 MCP 接入），有真实审核者后 |
| **复杂 diff**（层级影响面 / 变更波及分析） | MVP 只做增/删/改三类 | 规则层落地时（生产阶段，§6.5）—— 规则引擎先算影响面，diff 才能展示 |
| **`disjoint_with` 的校验逻辑** | 切片 1 它只是审核提示与导出内容，**不是推理公理**（§4.6） | 切片 2 之后（生产阶段），做数据写入校验 + OWL 导出 |
| **HTTP API / MCP server** | MVP 用 CLI，不对外暴露服务 | 切片 1 第 3 份计划 |
| **数据库迁移**（Alembic / Flyway） | MVP 用 `create_all` | **首次部署前** —— 不是「以后再说」，是上生产前必须做 |
| **权限 / 多租户运行时** | 使用者就是 FDE（§10.2） | 私有化交付时（§10.1）—— 客户内多人时 |
| **抽取与跨表合成** | 属第 2 份计划 | **第 2 份计划**（§7.1 阶段 0/1/3/3d/3e），已完成 spike 验证 |
| **写回执行** | 属切片 2 | **切片 2**（§5）。MVP 必须**不存在任何可执行写回路径**（有测试守住） |

> **唯一一条容易真被忘的**：`extracted_by` 不在本节 —— 它是**必须占位**（§2.4），不属暂缓。

---

## 4. 技术选型

| 项 | 选择 | 说明 |
| --- | --- | --- |
| 工具链 | Rust 1.86 | AFIT（async fn in trait）已稳定 |
| 异步 | `tokio` | 后续接 MCP / A2A 需要 |
| DB | `sqlx` 0.8 | 异步、生态活跃 |
| SQL 写法 | `sqlx::query_as`（**不用 `query!` 宏**） | 编译期校验 SQL 需要 DB 在线或 `.sqlx` 离线数据；**这轮 spike 已经吃过一次「凭空编列名」的亏**，但那是查询逻辑错，不是类型错 —— MVP 换来的是零构建依赖 |
| 序列化 | `serde` + `serde_json` | `Snapshot` 就是契约 |
| 校验 | 手写 `Result` + `thiserror` | 不引框架。领域不变量用类型表达 |
| CLI | `clap`（derive） | |
| 错误 | `thiserror`（域）+ `anyhow`（应用） | |
| 测试 | `cargo test` + `testcontainers` | 契约测试要**真 PG**，不用 mock |
| 迁移 | 无 | `create_all` |

**工程结构**（单 crate，不搞 workspace）：

```
src/
├── main.rs        CLI（clap）
├── domain/        纯逻辑，**不碰 I/O**，可单测
│   ├── snapshot.rs    快照契约（含全部占位字段）
│   ├── review.rs      双轨状态机
│   ├── view.rs        客户视图投影
│   ├── diff.rs        快照 diff
│   ├── apply.rs       提案应用与冻结
│   └── hierarchy.rs   层级纯函数（环检测 / 祖先 / 后代）
├── store/
│   └── pg.rs          PgStore（MVP 唯一实现）
└── db/
    ├── project.rs     Project / OntologyVersion
    ├── ontology.rs    ObjectType / Property / LinkType / ActionType / Function
    └── proposal.rs    Proposal / ProposalItem / ReviewRecord
```

> **分层纪律**：`domain/` 里出现 `sqlx` 即为违规。验收用一条命令守住（见 §6）。

---

## 5. 任务分解

| # | 任务 | 产出 | 依赖 |
| --- | --- | --- | --- |
| 1 | 骨架与连通 | `cargo init`、依赖、`PgStore::connect`、testcontainers fixture | — |
| 2 | Project + Version | 容器层 | 1 |
| 3 | 本体 schema 模型 | ObjectType / Property / LinkType + **ActionType / Function 占位** | 2 |
| 4 | 快照契约 | `Snapshot` 结构，**含 `extracted_by` 与全部占位** | 3 |
| 5 | 提案模型 | Proposal / ProposalItem / ReviewRecord | 3 |
| 6 | 双轨审核状态机 | 纯逻辑 | 5 |
| 7 | 客户视图投影 | 零建模术语 | 6 |
| 8 | 提案应用 + 冻结 | 链式累积、冻结拒写 | 6,7 |
| 9 | 快照 diff | 增/删/改三类 | 4,8 |
| 10 | CLI 端到端 | §1 那串命令跑通 | 1–9 |
| 11 | 层级纯函数 | 环检测 / 祖先 / 后代 | 4 |

**11 个任务，比 Python 计划的 12 个少，但每步更粗**（计划里 86 个 TDD 步骤 → 这里按任务推进，实施时再展开）。

---

## 6. 完成标准

全部可执行、可验证：

```bash
# 1. 测试全绿
cargo test

# 2. 领域层不碰 I/O —— 分层纪律
grep -rn 'sqlx\|PgStore\|db::' src/domain/ | grep -vE '\s*//' \
  && echo '✗ 违规' || echo '✓'

# 3. 切片 1 无写回路径
cargo test test_no_write_path_in_slice1

# 4. 客户视图零建模术语
cargo test test_biz_view_never_exposes_modeling_terms

# 5. 冻结后拒写
cargo test test_frozen_version_rejects_writes

# 6. 关系条目依赖两端被接受
cargo test test_link_acceptance_requires_both_ends_accepted

# 7. 占位字段全在快照契约里
grep -q 'action_types' src/domain/snapshot.rs && \
grep -q 'functions'    src/domain/snapshot.rs && \
grep -q 'extracted_by' src/domain/snapshot.rs && \
grep -q 'write_bindings' src/domain/snapshot.rs && echo '✓ 占位齐全'

# 8. 端到端可跑（§1 那串命令）
```

**外加一条人工判据**：拿 §1 跑出来的客户视图，问一句「**这看得懂吗、有没有明显错的**」。看不懂就是投影没做到位。

---

## 7. 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | **重写引入新缺陷** —— 3600 行逻辑没人跑过 | 领域逻辑逐个任务对照旧计划的定义；每完成一个任务就跑一次端到端 |
| 2 | sqlx 类型与 Pydantic 语义差异（`Optional`/`null`） | 快照往返测试（save → load → 断言相等），这是最强的一道 |
| 3 | 契约测试要真 PG，本机未必有 | testcontainers；退路是 CI 里起 PG |
| 4 | 占位字段「只建不用」容易被后人删掉 | 完成标准第 7 条用 `grep` 守住 |
| 5 | **MVP 通过 ≠ 客户认** | 与 spike 同理：内核是通用机制，抽得准不准是下一层的事。**不要用 MVP 通过去对客户承诺效果** |

---

## 8. 与 spike 的关系

| spike 产出 | 在 MVP 里的位置 |
| --- | --- |
| `core.py` 的**证据验证器**（~120 行 Python） | 属第 2 份计划（抽取侧），**不进本 MVP**。但它已实测有效，可作证据规则的参照 |
| `synthesize.py` 聚类 | 同上，第 2 份计划 |
| 阶段 0/1/3/3d/3e | 第 2 份计划 |
| 「每条提案必须带可验证证据」这条规则 | **✅ 进本 MVP** —— `evidence` / `source_column` 是快照契约的一部分 |

> **实测结论已落档设计规格 §7.1**：阶段 1 / 3d / 3e 经真实库验证**可外置**，阶段 2 代码验证**不可外包**。
> 本 MVP 做的就是阶段 2 那侧的**承接部分**：它接收的提案必须已经带证据，且证据已由抽取侧验证过。

---

## 9. 后续

- [x] 范围已确认（2026-09-30，§3 的 10 条为**暂缓**而非永久不做，每条已标注补回时机）
- [ ] 展开为逐步实施文档（像 Python 计划那样的 TDD 步骤）
- [ ] 建 `code/` worktree 的 Rust 骨架（Task 1）
- [ ] `sqlx::query_as` 不用编译期宏 —— 已确认
- [ ] MVP 里不留 A2A 客户端 trait 占位 —— 已确认（等第 2 份计划）

> ⚠ **范围变更提示**：若后续要提升 §3 中某一项为「永久不做」，**必须同步更新设计规格 §11**，否则两处语义会打架。
