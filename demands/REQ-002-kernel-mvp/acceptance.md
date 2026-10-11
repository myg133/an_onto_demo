# 验收标准：REQ-002 本体内核 MVP

> 全部**可执行、可验证**。命令形式的验收项，任何人 clone 下来就能复现。

---

## AC-001 · 测试全绿

- 前置条件：Rust 1.86+、**外部 PostgreSQL 16.14 可达**（`ONTOK_TEST_DATABASE_URL`）
  - **不用 testcontainers** —— 本机 Docker daemon 未起，且外部库更省事
  - 口令**不写入任何文档**，只走环境变量（见 `.env.example`）
- 测试步骤：`cargo test`
- 预期结果：0 failed
- 类型：功能

## AC-002 · 分层纪律：领域层零 I/O

- 测试步骤：
  **统一用判定器执行**（见 [`BA/verify.sh`](../../../verify.sh)）：

```bash
bash BA/verify.sh
```

> **为什么脚本化**：本 AC 曾三次因「命令本身写错」而误判 —— grep 匹配到文档里写的命令本身、
> `|| echo ✓` 吞掉退出码 2、测试名与实际不符。**判定器必须有「目录不存在 → 判定无效」的分支。**
- 预期结果：`✓`，`src/domain/` 内无任何 I/O 符号
- 类型：架构
- **不通过的处理**：把 I/O 挪回 `store/`，`domain/` 保持纯逻辑

> ## ⚠ 本文件所有「命令形式」的 AC 一律以判定器为准
>
> **不要直接复制本文件里的命令去跑** —— 它们是「意图说明」，
> 而 [`BA/verify.sh`](../../../verify.sh) 是**唯一可执行的判定入口**。
>
> 理由：本迭代已累计 **7 次判定误判**，其中 3 次源于命令本身写错
> （测试名与实际不符、target 指错测试文件、`pipefail` 吞掉退出码）。
> 每新增一条命令形式 AC，**先在判定器里落一条**，再让人照判定器跑 ——
> 不要维护两份会漂移的副本。
>
> 判定器区分**三态**：`✓ 通过` / `✗ 未通过` / `⚠ 未实现`
> （对应任务没开工，**不等于「写坏了」**）。

## AC-003 · 快照契约含全部占位

- 测试步骤：
  ```bash
  for f in action_types functions WriteBinding extracted_by failed_groups disjoint_with; do
    grep -q "$f" src/domain/snapshot.rs || echo "✗ 缺 $f"
  done
  ```
- 预期结果：无输出（全部就位）
- 类型：架构
- **理由**：漏任一项，对应切片必然推翻本 MVP 的数据结构（规格 §4.6 / §5.2 / §8.4 / §7.1⑤）

## AC-004 · 切片 1 无任何可执行写回路径

- 测试步骤：

  ```bash
  cargo test --tests test_no_write_path_in_slice1 2>&1 \
    | grep -qE '^test .* \.\.\. ok' \
    || echo '✗ 该测试不存在或未通过（零匹配也算失败）'
  ```
- 预期结果：无输出（测试确实存在且通过）
- 类型：安全
- **理由**：只读闭环的底线（规格 §9.1）

## AC-005 · 客户视图零建模术语

- 测试步骤：

  ```bash
  cargo test --tests test_view_biz_view_never_exposes_modeling_terms 2>&1 \
    | grep -qE '^test .* \.\.\. ok' \
    || echo '✗ 该测试不存在或未通过（零匹配也算失败）'
  ```
- 预期结果：无输出（测试确实存在且通过） —— 序列化结果中不出现 `pk` / `cardinality` / `外键` / `基数` 等词
- 类型：功能
- **理由**：客户读不懂 → 审核流于形式（规格 §8.3）

## AC-006 · 冻结后拒绝一切写入

- 测试步骤：

  ```bash
  cargo test --tests test_frozen_version_rejects_writes 2>&1 \
    | grep -qE '^test .* \.\.\. ok' \
    || echo '✗ 该测试不存在或未通过（零匹配也算失败）'
  ```
- 预期结果：无输出（测试确实存在且通过）
- 类型：安全

## AC-007 · 关系条目依赖两端已被接受

- 测试步骤：

  ```bash
  cargo test --tests test_review_link_acceptance_requires_both_ends_accepted 2>&1 \
    | grep -qE '^test .* \.\.\. ok' \
    || echo '✗ 该测试不存在或未通过（零匹配也算失败）'
  ```
- 预期结果：无输出（测试确实存在且通过） —— 关系类条目在两端任一未被接受时必须被拦截
- 类型：功能
- **理由**：否则产生引用了不存在实体的野指针

## AC-008 · 链式 apply 累积而不重置

- 测试步骤：

  ```bash
  cargo test --tests test_chained_apply_keeps_accumulating 2>&1 \
    | grep -qE '^test .* \.\.\. ok' \
    || echo '✗ 该测试不存在或未通过（零匹配也算失败）'
  ```
- 预期结果：无输出（测试确实存在且通过）
- 类型：功能

## AC-009 · 层级：环检测与悬空父引用

- 测试步骤：

  ```bash
  cargo test --tests test_hierarchy 2>&1 \
    | grep -qE '^test .* \.\.\. ok' \
    || echo '✗ 该测试不存在或未通过（零匹配也算失败）'
  ```
- 预期结果：无输出（测试确实存在且通过） —— 环必须被检出；父类型不存在必须被检出
- 类型：功能

## AC-010 · 快照往返无损

- 测试步骤：

  ```bash
  cargo test --tests test_snapshot_roundtrip 2>&1 \
    | grep -qE '^test .* \.\.\. ok' \
    || echo '✗ 该测试不存在或未通过（零匹配也算失败）'
  ```
- 预期结果：无输出（测试确实存在且通过） —— `save → load` 后与原对象逐字段相等，**含 `None` / 空列表 / 空 JSON 三态**
- 类型：功能
- **理由**：serde 与 sqlx 的 null 语义最容易在这里出错；这是最强的一道防线

## AC-011 · `extracted_by` 记录 agent 与其具体版本

- 测试步骤：

  ```bash
  cargo test --tests test_snapshot_records_extracted_by 2>&1 \
    | grep -qE '^test .* \.\.\. ok' \
    || echo '✗ 该测试不存在或未通过（零匹配也算失败）'
  ```
- 预期结果：无输出（测试确实存在且通过） —— 快照能区分「同一本体、不同 agent 版本」
- 类型：功能
- **理由**：否则供应商悄悄升模型后，diff 变化无法解释（规格 §8.4）

## AC-012 · 端到端可跑（人工）

- 前置条件：`fixtures/proposal-demo.json` 就位
- 测试步骤：依次执行
  ```bash
  export ONTOK_TEST_DATABASE_URL="postgresql://…"   # 口令不入库
  ontok project new --name acme
  ontok version new  --project acme
  ontok propose import --file fixtures/proposal-demo.json
  # demo 提案有 **6 个 item**：客户 / 订阅 / 资产 / 设备(parent=asset) /
  # 客户—订阅 / 资产—设备。**必须全部审核**：
  # 只审一部分的话，未审的条目是 Pending → 整体 passed=false → apply 被拒。
  # 关系类（5、6）还要求两端（1~4）先被接受，否则被 AC-007 拦成悬空。
  for i in 1 2 3 4 5 6; do
    ontok review decide --proposal p1 --item $i --track biz   --accept
    ontok review decide --proposal p1 --item $i --track model --accept
  done
  ontok review show    --proposal p1            # 整体通过：是
  ontok proposal apply --proposal p1            # ← 提案进入版本，产生 v2
  ontok version freeze --project acme --version 2
  ontok version list   --project acme
  ontok ontology show  --project acme --version 2   # 默认客户视图
  ontok ontology diff  --project acme 1 2
  ```
  > **`--proposal p1` 的别名按「创建时间倒序」解析** —— `p1` 恒等于刚导入的那个，
  > 与测试库里的历史数据无关。（正序会指向老数据，演示命令作用在别人的数据上
  > 而且照样「成功」。）
- 预期结果：**16 条命令全部成功**（含 12 条 `review decide`）；apply 后 v2 含
  **客户、订阅、资产、设备 4 个对象 + 2 条关系**；`diff 1 2` 报 6 处新增；
  `ontology show` 的客户视图零建模术语
- **额外验证（冻结是判据）**：v2 冻结后再 import 一份以 v2 为 base 的提案
  → 应报「base 版本 v2 已冻结」并以非 0 退出
- 类型：功能

> **已实跑（2026-09-30）**：全序列通过。实现过程中该序列抓出两个真 bug ——
> ① `available` 集合混用展示名与系统 key，导致关系全被判悬空；
> ② 提案别名按正序解析到历史老数据。详见 `BA/dispatch/registry.md`。

## AC-013 · 客户视图可读（人工判据）

- 前置条件：AC-012 完成
- 前置条件：指定 3 名评审人，**其中不得为本次开发者**
- 测试步骤：执行 `ontok ontology show --version 2`，每人回答两问
  1. 「这些是你们的什么东西？」
  2. 「它有哪几个属性？」
- 预期结果：**至少 2/3 人能答出第 1 问且答对 ≥ 1 个属性**
- 判定：2 人通过 = 通过；1 人通过 = 不通过（投影没做到位）
- 类型：功能
- **不通过的处理**：投影没做到位，改 `domain/view.rs`，不要改测试

---

## 覆盖对照

| 本 AC | 对应范围规划 | 对应规格 |
|---|---|---|
| AC-002 | §4 工程结构 | §4.6 唯一数据契约 |
| AC-003 | §2.4 必须占位 | §4.6 / §5.2 / §8.4 / §7.1⑤ |
| AC-004 | §3 暂缓「写回执行」 | §9.1 |
| AC-005 | §2.3 客户视图 | §8.3 |
| AC-007 | §2.3 双轨审核 | §8.3 |
| AC-011 | §2.4 `extracted_by` | §8.4 |
| AC-004 | §3 暂缓「写回执行」 | §9.1 |
| AC-006 | §2.3 提案应用+冻结 | §8.4 |
| AC-008 | §2.3 提案应用 | §8.4 |
| AC-009 | §5 Task 11 层级 | §4.6 |
| AC-010 | §4 工程结构（三层） | §4.6 唯一数据契约 |
| AC-012 | §1 MVP 判据 | §2.2 / §2.3 |
| AC-013 | §1 MVP 判据 | §2.2 / §2.3 |

---

## AC-014 · `extracted_by` 按 ADR-001 取值

- 前置条件：一条带 `extracted_by` 的提案；一条不带该字段的提案
- 测试步骤：

  ```bash
  cargo test --tests test_extracted_by_is_not_inherited 2>&1 \
    | grep -qE '^test .* \.\.\. ok' \
    || echo '✗ 该测试不存在或未通过'
  ```
- 预期结果：PASS —— 验证三件事
  1. 提案带 `extracted_by` 时，新版本记录该值
  2. 提案**不带**时，新版本记 `None`（人工修订）
  3. **新版本绝不继承 base 的值** —— 这是 ADR-001 的核心，防止 agent 换人不可见
- 类型：功能 / 架构
- **理由**：规格 §8.4。继承会让「agent 偷偷升级 → diff 无法解释」重新发生
- 决策依据：[`decisions/ADR-001-extracted-by-policy.md`](../../decisions/ADR-001-extracted-by-policy.md)

## AC-015 · `failed_groups` 不继承

- 测试步骤：

  ```bash
  cargo test --tests test_failed_groups_not_inherited 2>&1 \
    | grep -qE '^test .* \.\.\. ok' \
    || echo '✗ 该测试不存在或未通过'
  ```
- 预期结果：PASS —— 每轮 `failed_groups` 独立记录，不从 base 继承
- 类型：功能
- **理由**：规格 §7.1⑤。继承会让「上轮失败的组」被误认为「本轮已覆盖」→ 静默失败伪造收敛
