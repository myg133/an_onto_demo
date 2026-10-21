# 验收标准：REQ-108 · 带证据与置信度的关系抽取

> **代码基线**：`develop` `4573269`
> **实测依据**：2026-10-21 用真实库 `anspire_open_uat`（96 表 / 0 外键 / 81 条推断关系）跑出的采样结果
> **本机限制**：全量 `cargo test` 在 Windows 编不过（`src/bin/ontok-gate-preflight.rs` 的 `std::os::unix`，既有问题）⇒ 交付物必须声明跑的是哪些目标

---

## 零 · 本需求的「已知错误集」（AC-006 的判据来源）

这 9 条关系已用 5 行采样实测为错，**必须全部落进「拒绝」档**：

| # | 关系 | 实测 | 错误类型 |
| --- | --- | --- | --- |
| E1 | `balance_operation_log.batch_id → gift_code_batch.id` | 0/5 | 同名不同义 |
| E2 | `instance_manager.session_id → simulate_login_sessions.id` | 0/5 | 同名不同义 |
| E3 | `internal_operation_records.batch_id → gift_code_batch.id` | 0/3 | 同名不同义 |
| E4 | `llm_model_peak_config.model_id → llm_model.id` | 0/4 | 目标列选错（应为 `llm_model.model_id`） |
| E5 | `llm_model_sort.model_id → llm_model.id` | 0/5 | 同上 |
| E6 | `llm_provider.model_id → llm_model.id` | 0/5 | 同上 |
| E7 | `llm_provider_groups.model_id → llm_model.id` | 0/5 | 同上 |
| E8 | `dim_customer_binding.channel_code → marketing_channel.id` | 类型 text→int | 类型不兼容 |
| E9 | `marketing_channel_customer.channel_code → marketing_channel.id` | 类型 text→int | 类型不兼容 |

另有 **5 条部分命中**（1/5 – 4/5），**不得落进「高」档**（允许「中」或「低」）：
`dim_customer_binding.customer_id`（4/5）· `enterprise_recharge_order.customer_id`（1/5）· `order.subscription_id`（1/2）· `service_apps.channel_id`（4/5）· `usage_reconciliation_log.subscription_id`（3/5）

---

## 一 · AC

### AC-001 · 勘查补采主键与唯一索引

- `ontok-introspect` 产物里每张表带 `primary_key`（列名或 null）与 `unique_columns`（数组）
- 判据：以 `anspire_open_uat` 为例，`llm_model.primary_key == "id"`；`marketing_channel.id` 在 `unique_columns` 里
- ⛔ 不许用「列名是不是 `id`」来猜——那正是本需求要消除的近似

### AC-002 · 采样是只读的、且**确定性**

- 连接后 `SHOW transaction_read_only` 必须为 `on`，且只发 `SELECT`
- **同一份数据两次运行，抽到的样本必须完全相同**（`ORDER BY 主键 LIMIT 5`，⛔ 不用 `TABLESAMPLE` / `random()`）
- 判据：两次运行的 `signals.sample_values_digest` 相同（**只存样本的哈希，不存原值**）

### AC-003 · 采样结果**不落原值**

- 产物（`proposal.json` / `stats.json` / 日志）中**不得出现任何业务数据值**
- 只允许：`sampled`（样本数）、`matched`（命中数）、`sample_digest`（样本哈希）
- 判据：对产物做一次「采样值回查」——把采样到的 5 个值拿去 grep 产物，**命中数必须为 0**

### AC-004 · veto 上限优先于加权和

构造四类样本，逐类断言上限生效：

| 类型 | 期望上限 |
| --- | --- |
| 类型族不相容（int vs text） | ≤ 0.15 |
| 采样 0 命中 | ≤ 0.20 |
| 泛化列名 + 无数据证据 | ≤ 0.40 |
| 源列全空（`sampled = 0`） | ≤ 0.45 |

判据：把 `name_match` / `target_is_key` 全部人为置 1.0，**分数仍不得超过上限**（这条证明 veto 没被加权和救回来）。

### AC-005 · 评分可复算，且不改动既有抽取产物

- 同一 schema + 同一数据跑两次：`confidence.score` **逐位相同**
- **`items` 逐字节不变**（REQ-059 口径：`extracted_by` 除外）
- 判据：`git diff` 两次产物的 `items` 段为空；`confidence` 段为空差异

### AC-006 · 9 条已知错误**全部**落「拒绝」（< 0.30）

- 见 §零 的 E1–E9
- 判据：一条不落即 FAIL。**这是本需求最核心、最可证伪的一条。**

### AC-007 · 5 条部分命中不得落「高」

- 见 §零 末尾
- 判据：5 条中任一条 `band == 高` 即 FAIL

### AC-008 · `method` 必填

- 每个 `confidence` 必须带 `method`（如 `evidence-scorecard-v1`）
- 缺 `method` 的条目：**拒绝入库**，并报错说明「没有 method 的分数不可复核」
- 判据：构造一条缺 `method` 的条目，`propose import` 必须非 0 退出

### AC-009 · 分档路由生效

| 分数 | 行为 |
| --- | --- |
| ≥ 0.90 | 进提案，审核界面默认折叠 |
| 0.60–0.90 | 进提案，逐条显示 |
| 0.30–0.60 | 进「待查清单」，**不进主提案** |
| < 0.30 | **不提案**，只记拒绝理由 |

判据：用真实库跑一遍，统计四档条数，且「不提案」档里的关系在 `proposal.json` 中**一条都找不到**。

### AC-010 · 客户视图零泄漏

- `ontology show`（客户视图 JSON / HTML）**不得出现** `confidence` / `score` / `band` 任何字段
- `--model-view` 与审核界面**必须**有
- 判据：客户视图产物 grep 上述字段 = 0；model-view 里每条关系都有

### AC-011 · 校准命令

- 提供一条命令输出**分箱命中率**（如 `0.9–1.0: 41 条 / 人工接受 39 → 95%`）
- 样本不足时必须报「样本不足」，⛔ 不许输出 `0%` 或 `100%` 冒充结论
- 判据：空库运行 → 明确提示样本不足；有数据运行 → 输出分箱表

### AC-012 · 红线与不退化

| 项 | 约束 |
| --- | --- |
| 禁改 | `tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs` / `src/domain/review_view.rs` / `src/mcp/tools.rs` / `mcp::tools::guard()` 内部 |
| 禁改 | `property_filter.rs` 的 G1–G4 措辞 |
| 禁 | 删测试、`#[ignore]`、放宽断言、新增依赖（不引新 crate） |
| 不许退化 | REQ-107 的 13 条出口判据（74 passed）不得变红 |

### AC-013 · 交付证据

1. 评分卡规格与权重表（含「为什么是这个权重」）
2. 81 条真实关系的**分档分布**实测
3. 9 条已知错误全部落「拒绝」的实测输出
4. 采样只读 + 确定性的证据（`SHOW transaction_read_only` + 两次样本 digest 相同）
5. 不落原值的证据（采样值回查命中 0）
6. `items` 未变的证据（逐字节 diff）
7. 全量/局部测试数字 + 口径 + **本机不可跑全量**的声明
8. 已知限制

---

## 二 · 纪律（逐条适用）

- **E** · 扫描报「零」先怀疑扫描器；「跑不出」先怀疑环境
- **G** · 变异三元组（二进制 sha256 + patch + 逐条结果）；⛔ 编译失败不算执行
- **H** · 一致性守卫抓不到「忠实的重复实现」；不许写第二份判据
- **I** · 每条命令自带 `cd` 或全用绝对路径
- **J** · ⛔ 不要 `2>/dev/null`
- **L** · 报「环境不具备」前先查变量来源
- **S** · **AC 自己也会是错的** —— 发现某条不成立，直接说，不要迁就实现
- **临时文件** · 只放 `.review-workspace/req108/`；⛔ 禁 `rm`
- **源库** · ⛔ **只读**（`SET default_transaction_read_only = on`，只 SELECT，禁一切写操作）
