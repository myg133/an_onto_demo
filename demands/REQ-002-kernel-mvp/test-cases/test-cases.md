# REQ-002 测试用例

> **需求**：[REQ-002 本体内核 MVP（Rust）实施](../demand.md)
> **验收标准**：[acceptance.md](../acceptance.md)（15 条 AC）
> **实施计划**：[2026-09-30-REQ-002-kernel-mvp-implementation.md](../../../../docs/plans/2026-09-30-REQ-002-kernel-mvp-implementation.md)
> **编写**：QA Agent · 2026-09-30
> **代码位置**：`feature-REQ-002/` worktree（分支 `feature/REQ-002`）

---

## 0 · 使用说明

### 0.1 环境

| 项 | 值 |
| --- | --- |
| 工作目录 | `feature-REQ-002/`（worktree 根，非 `BA/` worktree） |
| 工具链 | Rust **1.86+**（实施计划 §环境） |
| 数据库 | **外部 PostgreSQL 16.14**，**不用 testcontainers** |
| 环境变量 | `export ONTOK_TEST_DATABASE_URL="postgresql://…@<host>:<port>/ontol"` |
| 迁移 | 无，用 `create_all` |
| 数据隔离 | 各测试用**独立 project name**（`t_acme_1`…），**不 truncate**（外部库多人共用） |

> ⚠ **AC-001 的前置条件写的是「Docker（供 testcontainers 起真 PG）」，与实施计划「不用 testcontainers，用外部 PG」矛盾。**
> 本文档一律**以实施计划为准**（外部 PG）。详见 §3 缺陷 D-01。

### 0.2 三个已知的假通过 / 假失败陷阱（务必沿用）

| 陷阱 | 现象 | 正确写法 |
| --- | --- | --- |
| **T-1 `--tests` 不能省** | `cargo test <filter>` 只跑 lib 单元测试；集成测试零匹配 → **退出 0 假通过** | 一律 `cargo test --tests <filter>` |
| **T-2 AC-002 必排除注释** | 文档里写这条 grep 命令本身会被自己命中 → **假失败** | `\| grep -vE '\s*//'` |
| **T-3 零匹配 = 失败** | 测试名拼错 / 改名 → 零匹配 → 判定器必须报 ✗ | `grep -qE '^test .* \.\.\. ok' \|\| echo '✗'` |

> **测试名分组前缀**（便于过滤与定位）：
> `test_hierarchy_`（`tests/test_hierarchy.rs`）·
> `test_review_`（`tests/test_review_state.rs`）·
> `test_view_`（`tests/test_view.rs`）

### 0.3 反向测试的标准做法

所有「反向 / 判定器有效性」用例都在 `feature-REQ-002/` worktree 内**临时改动 → 跑验收命令 → 立即还原**：

```bash
# 1. 造违规        2. 跑判定      3. 还原
git -C feature-REQ-002 status --porcelain   # 起点必须干净
# ...临时修改...
cargo test --tests <filter> ; echo "exit=$?"
git -C feature-REQ-002 checkout -- <改动的文件>
git -C feature-REQ-002 status --porcelain   # 必须回到空
```

**判定标准**：反向用例「通过」= **验收命令报了 ✗**（而不是「测试全绿」）。
判定器没报错 → 该红线**形同虚设** → 判该 AC **不通过**。

### 0.4 用例编号规则

`TC-<AC 序号>-<序号>`，例：`TC-010-04` = AC-010 的第 4 条。
优先级：**P0** = 红线 / MVP 判据，必跑；**P1** = 边界与判定器自检，必跑但可后置；**P2** = 人工与环境类。

---

## AC-001 · 测试全绿

### TC-001-01 · 全套件可编译且 0 failed
- 关联验收：AC-001
- 前置条件：`ONTOK_TEST_DATABASE_URL` 指向可达的 PG 16；worktree 干净
- 步骤：
  1. `cd feature-REQ-002`
  2. `export ONTOK_TEST_DATABASE_URL="…"`
  3. `cargo test 2>&1 | tee /tmp/req002-cargo-test.log`
  4. `grep -E 'test result:' /tmp/req002-cargo-test.log`
- 预期：输出行中 **`0 failed`** 一律出现；无 `error[E….]`；退出码 0
- 类型：功能
- 优先级：**P0**

### TC-001-02 · 测试环境可达且版本达标
- 关联验收：AC-001
- 前置条件：无
- 步骤：
  1. `cargo test --test test_db 2>&1 | grep -qE '^test .* \.\.\. ok' && echo ✓`
  2. 连库查版本：`psql "$ONTOK_TEST_DATABASE_URL" -c 'SHOW server_version;'`
- 预期：第 1 步输出 `✓`；第 2 步返回 **16.x**
- 判读：若第 1 步输出 ✗ 而**不是**「无测试可跑」，说明环境不通 → **不得**把后续 DB 类 AC 判为通过
- 类型：功能
- 优先级：**P0**

### TC-001-03 · `--tests` 确实覆盖了全部集成测试文件（陷阱 T-1 自检）
- 关联验收：AC-001
- 前置条件：全部 Task 已完成
- 步骤：
  1. `ls feature-REQ-002/tests/*.rs | wc -l` → 记为 `N`
  2. `cargo test --tests 2>&1 | grep -c 'Running tests/'` → 记为 `M`
  3. 反向验证：把 `tests/test_diff.rs` 临时改名为 `tests/test_diff_off.rs`，重跑第 2 步
  4. `git -C feature-REQ-002 checkout -- tests/`
- 预期：`M == N`；第 3 步 `M` 应减 1（证明过滤器真的在数集成测试二进制，不是恒等）
- 判读：`M < N` → 某些测试文件被静默漏跑 → AC-001 **不通过**
- 类型：功能
- 优先级：**P0**

---

## AC-002 · 分层纪律：领域层零 I/O 【红线】

> 判定命令（沿用陷阱 T-2 的写法）：
> ```bash
> grep -rn 'sqlx\|PgStore\|db::' src/domain/ \
>   | grep -vE '\s*//' && echo '✗ 违规' || echo '✓'
> ```

### TC-002-01 · 正向：领域层无任何 I/O 符号
- 关联验收：AC-002
- 前置条件：全部 Task 已完成
- 步骤：在 `feature-REQ-002/` 根跑上方判定命令
- 预期：输出 `✓`（`grep -rn` 零输出）
- 类型：架构
- 优先级：**P0**

### TC-002-02 · 反向：加了真实 I/O 调用必须被抓
- 关联验收：AC-002
- 前置条件：worktree 干净
- 步骤：
  1. 在 `src/domain/view.rs` 末尾临时追加一行**无注释**的 `let _p = sqlx::PgPool::connect("…");`
  2. 跑判定命令
  3. `git -C feature-REQ-002 checkout -- src/domain/view.rs`
- 预期：第 2 步输出 **`✗ 违规`**
- 判读：输出 `✓` → 判定器失效 → AC-002 **不通过**
- 类型：架构（反向）
- 优先级：**P0**

### TC-002-03 · 判定器自检：注释行不算违规（陷阱 T-2）
- 关联验收：AC-002
- 步骤：
  1. 临时在 `src/domain/hierarchy.rs` 追加 `// 将来也许要 db::load()`
  2. 跑判定命令
  3. 还原
- 预期：输出 **`✓`**（证明 `grep -vE '\s*//'` 生效，文档里写这条命令不会假失败）
- 类型：架构（判定器自检）
- 优先级：**P0**

### TC-002-04 · 判定器盲区探测：代码 + 行尾注释会被漏掉
- 关联验收：AC-002
- 步骤：
  1. 临时在 `src/domain/review.rs` 追加 `let _s = crate::db::Store; // 暂时借用`
  2. 跑判定命令
  3. 还原
- 预期（按当前判定器）：输出 **`✓`** —— 即**这条违规不会被抓到**
- 判读：本用例的作用是**如实记录判定器盲区**。该行含 `db::`，本应判违规，但 `grep -vE '\s*//'` 把整行（含代码）一起剔除了。
  **这是 AC-002 判定器的已知假通过路径**（见 §3 缺陷 D-02）。修复方向：把命令换成「去掉行尾注释后再匹配」，例如
  `grep -rn … | sed -E 's@//.*@@' | grep …`，或改用 `grep -rn --include='*.rs' -e … ` 配合逐行判前缀。
  **在修好之前，AC-002 的「✓」不足以单独作为架构合规的证据**，需人工复核 `src/domain/` 三个文件。
- 类型：架构（判定器自检）
- 优先级：**P0**

### TC-002-05 · 目录缺失 / 空目录的判定行为
- 关联验收：AC-002
- 步骤：
  1. `mv src/domain src/domain_bak`
  2. 跑判定命令
  3. `mv src/domain_bak src/domain`
- 预期（当前）：输出 `✓` —— `grep -rn` 对不存在的目录返回退出码 2，被 `||` 吞成 `✓`
- 判读：**目录整体消失也算「通过」= 假通过**（见 §3 缺陷 D-03）。修复方向：先 `test -d src/domain || { echo '✗ 目录缺失'; exit 1; }`。
  本用例的通过标准是「**如实复现了假通过并被记录**」，而不是「判定器没问题」。
- 类型：架构（判定器自检）
- 优先级：**P1**

### TC-002-06 · 正向佐证：I/O 确实落在 store / db 层
- 关联验收：AC-002
- 步骤：
  1. `grep -rln 'sqlx' src/store/ src/db/ 2>/dev/null`
- 预期：至少列出 1 个文件；即 I/O **不是**被删掉了，而是被放在 `src/domain/` 之外
- 判读：若 AC-002 通过但本条零输出 → 说明根本没有持久化实现，AC-002 的通过无意义
- 类型：架构
- 优先级：**P0**

---

## AC-003 · 快照契约含全部占位 【红线】

> 判定命令：
> ```bash
> for f in action_types functions WriteBinding extracted_by failed_groups disjoint_with; do
>   grep -q "$f" src/domain/snapshot.rs || echo "✗ 缺 $f"
> done
> ```

### TC-003-01 · 正向：6 项占位全部就位
- 关联验收：AC-003
- 前置条件：Task 4 已完成
- 步骤：跑判定命令
- 预期：**无输出**
- 类型：架构
- 优先级：**P0**

### TC-003-02 · 反向：删掉任一占位必须被抓
- 关联验收：AC-003
- 步骤：
  1. 临时把 `src/domain/snapshot.rs` 里的 `functions` 字段整行注释掉
  2. 跑判定命令
  3. 还原
- 预期：输出 **`✗ 缺 functions`**
- 判读：无输出 → 判定器失效 → AC-003 **不通过**
- 类型：架构（反向）
- 优先级：**P0**

### TC-003-03 · 判定器自检：注释里写占位名不算数
- 关联验收：AC-003
- 步骤：
  1. 在 `src/domain/snapshot.rs` **首行**加注释 `// functions 属于切片 2，暂缓`
  2. **不改动**任何真实字段
  3. 跑判定命令
- 预期：**无输出**（6 项都由真实代码满足）
- 判读：若把 `functions` 真实字段删掉、只留这行注释后判定命令**无输出** → 说明 AC-003 判定器**会被注释满足**（与 AC-002 排除注释的做法不一致，见 §3 缺陷 D-04）
- 类型：架构（判定器自检）
- 优先级：**P0**

### TC-003-04 · WriteBinding 内部 4 个子字段都在
- 关联验收：AC-003（对应规格 §5.2）
- 步骤：
  1. 逐个确认 `src/domain/snapshot.rs` 的 `WriteBinding` 定义含
     `key_expr` / `value_map` / `supported_values` / `conflict_field`：
     `grep -n -e key_expr -e value_map -e supported_values -e conflict_field src/domain/snapshot.rs`
  2. 反向：临时删掉 `conflict_field` → 重跑第 1 步
- 预期：第 1 步 4 项各至少命中一次；第 2 步 `conflict_field` 零命中
- 判读：AC-003 的清单只查 `WriteBinding` **类型名**，内部 4 字段被后人清理掉**不会**被抓（见 §3 缺陷 D-05）
- 类型：架构
- 优先级：**P0**

### TC-003-05 · 占位不只是「名字在」—— 语义必须可用
- 关联验收：AC-003
- 步骤：审阅 `src/domain/snapshot.rs` 中 `WriteBinding.supported_values`
- 预期：`supported_values` 为**可空集合**类型（`Vec<String>`），且能表达空列表
- 判读：若为 `Option<Vec<..>>` 或被硬编码为非空 → 「不支持」这一有效表达无法表达 → 切片 2 需推翻
- 类型：架构
- 优先级：**P1**

### TC-003-06 · `extracted_by` 本身可空
- 关联验收：AC-003 + ADR-001 后果 1
- 步骤：`grep -n 'extracted_by' src/domain/snapshot.rs`；确认其类型为可空（`Option<…>`）
- 预期：快照的 `extracted_by` 字段**可空**（`None` = 人工修订，见 ADR-001）
- 判读：若为非空类型 → 人工修订版本无法表示 → 与 ADR-001 矛盾
- 类型：架构
- 优先级：**P0**

---

## AC-004 · 切片 1 无任何可执行写回路径 【红线 · 安全】

> 判定命令：
> ```bash
> cargo test --tests test_no_write_path_in_slice1 2>&1 \
>   | grep -qE '^test .* \.\.\. ok' || echo '✗ 该测试不存在或未通过（零匹配也算失败）'
> ```

### TC-004-01 · 正向：守门测试存在且通过
- 关联验收：AC-004
- 前置条件：全部 Task 已完成
- 步骤：跑判定命令
- 预期：**无输出**
- 类型：安全
- 优先级：**P0**

### TC-004-02 · 判定器自检（陷阱 T-3）：测试名改了必须被抓
- 关联验收：AC-004
- 步骤：
  1. 把该测试函数临时改名（去掉 `test_no_write_path_in_slice1` 中的 `no_write_path`）
  2. 跑判定命令
  3. 还原
- 预期：输出 **`✗ 该测试不存在或未通过（零匹配也算失败）`**
- 判读：输出 `✓` 状态的「无输出」→ 零匹配被判成通过 → 陷阱 T-1/T-3 复发 → AC-004 **不通过**
- 类型：安全（判定器自检）
- 优先级：**P0**

### TC-004-03 · 判定器自检：测试真的 failed 时必须被抓
- 关联验收：AC-004
- 步骤：
  1. 临时在守门测试体内加一行反向断言（断言「存在可执行写回函数」为真）
  2. 跑判定命令
  3. 还原
- 预期：输出 **`✗ …`**
- 判读：判定器只认 `^test .* \.\.\. ok` 行；`FAILED` 行与编译错误都不匹配 → fail-closed
- 类型：安全（判定器自检）
- 优先级：**P0**

### TC-004-04 · 守门测试不是空壳
- 关联验收：AC-004
- 步骤：
  1. `grep -n -A20 'test_no_write_path_in_slice1' tests/*.rs`
  2. 审阅该函数体：至少有一处**实质性断言**（计数为 0 / 某个可执行入口不存在），而非 `assert!(true)`
- 预期：函数体含具体断言
- 判读：AC-004 的判定器**只验「测试存在且 ok」，验不出空壳测试**（见 §3 缺陷 D-06）。本用例是人工兜底
- 类型：安全
- 优先级：**P0**

### TC-004-05 · 穷举探测：CLI 无任何写回子命令
- 关联验收：AC-004
- 步骤：
  1. `cargo run -- --help`
  2. 逐个子命令 `--help`，确认**不存在**面向业务系统执行写回的入口
     （如 `write`、`apply-write`、`execute`、`sync-to` 之类）
  3. 允许存在的写入口**仅限**本体内部：写快照 / 写提案 / 写审核决定
- 预期：无任何把 `WriteBinding` 送到外部系统的子命令
- 类型：安全
- 优先级：**P0**

### TC-004-06 · `WriteBinding` 只被序列化，不被执行
- 关联验收：AC-004
- 步骤：`grep -rn 'WriteBinding' src/`
- 预期：`WriteBinding` 的出现位置**仅限**：类型定义 · serde 派生/序列化 · 快照存取（`src/store/`）
- 判读：若出现「遍历 `effects` 逐条发请求 / 逐条执行 SQL」的代码 → 切片 1 已有可执行写回路径 → AC-004 **不通过**
- 类型：安全
- 优先级：**P0**

---

## AC-005 · 客户视图零建模术语 【红线】

> 判定命令：
> ```bash
> cargo test --tests test_biz_view_never_exposes_modeling_terms 2>&1 \
>   | grep -qE '^test .* \.\.\. ok' || echo '✗ 该测试不存在或未通过（零匹配也算失败）'
> ```
> ⚠ 本命令的 filter 与**实际测试名不一致**，见 TC-005-05。

### TC-005-01 · 正向：客户视图零建模术语
- 关联验收：AC-005
- 前置条件：Task 7 已完成
- 步骤：
  1. 跑判定命令
  2. 定位守门测试：`grep -n 'modeling_terms' tests/test_view.rs`
- 预期：第 1 步无输出；第 2 步命中 `tests/test_view.rs:69` 的 `test_view_biz_view_never_exposes_modeling_terms`
- 类型：功能
- 优先级：**P0**

### TC-005-02 · 反向：真的泄露术语必须被抓
- 关联验收：AC-005
- 步骤：
  1. 临时在 `src/domain/view.rs` 的客户视图结构里加一个 `cardinality` 字段
  2. 跑判定命令
  3. 还原
- 预期：输出 **`✗ …`**
- 判读：黑名单扫描器（`scan_modeling_terms`）应报出 `cardinality`
- 类型：功能（反向）
- 优先级：**P0**

### TC-005-03 · 判定器自检（陷阱 T-3）：filter 拼错必须被抓
- 关联验收：AC-005
- 步骤：
  1. 临时把 `test_view_biz_view_never_exposes_modeling_terms` 改名为 `test_view_terms`
  2. 跑判定命令
  3. 还原
- 预期：输出 **`✗ …`**
- 判读：零匹配 → 无 `ok` 行 → 必须报 ✗
- 类型：功能（判定器自检）
- 优先级：**P0**

### TC-005-04 · 黑名单扫描器本身有效
- 关联验收：AC-005
- 步骤：`cargo test --tests test_view_blacklist_scanner_catches_a_planted_term`
- 预期：命中 `tests/test_view.rs:167`，**通过**
- 判读：该测试往 JSON 里植入 `"cardinality":"1:N"`，断言扫描器能抓到。
  **这是「守门器守门器」** —— 证明黑名单不是空表。空黑名单会让所有 AC-005 用例假通过
- 类型：功能
- 优先级：**P0**

### TC-005-05 · ⚠ 验收命令与实际测试名一致性核查
- 关联验收：AC-005
- 步骤：
  1. `cargo test --tests test_biz_view_never_exposes_modeling_terms 2>&1 | tail -5`
- 实际观察：过滤串 `test_biz_view_never_exposes_modeling_terms` **不是**
  `test_view_biz_view_never_exposes_modeling_terms` 的子串（实际名多一个 `view_` 段）→ **零匹配**
- 预期（应为）：`running 0 tests` → 判定命令输出 **✗（假失败）**
- 判读：**AC-005 当前无法通过**。修法二选一：
  - 改命令为 `cargo test --tests test_view_biz_view_never_exposes_modeling_terms`（推荐，贴合分组前缀约定）；或
  - 把测试改名为 `test_biz_view_never_exposes_modeling_terms`
  本用例**在修复前判定为 AC-005 不通过**。见 §3 缺陷 D-07
- 类型：功能
- 优先级：**P0**

### TC-005-06 · 保留字段不得进入客户视图
- 关联验收：AC-005
- 步骤：`cargo test --tests test_view_omits_reserved_fields`
- 预期：命中 `tests/test_view.rs:77`，通过
- 判读：顶层 `action_types` / `functions` / `extracted_by` / `evidence` / `ordinal` / `key` /
  对象层 `key` / `evidence` / `disjoint_with` 全部不得出现
- 类型：功能
- 优先级：**P0**

### TC-005-07 · 术语黑名单覆盖完整
- 关联验收：AC-005
- 步骤：审阅 `src/domain/view.rs` 中 `scan_modeling_terms` 的黑名单常量
- 预期：至少含 `key` / `primary_key` / `pk` / `fk` / `cardinality` / `ordinal` /
  `外键` / `基数` / `主键`（对照规格 §8.3）
- 判读：黑名单缺项 → 对应词泄露不被抓（当前实现已含 `test_view_omits_reserved_fields`
  从结构层兜底，但**值层**仍依赖黑名单）
- 类型：功能
- 优先级：**P1**

### TC-005-08 · 上层对象显示人名而非 key
- 关联验收：AC-005
- 步骤：`cargo test --tests test_view_parent_is_shown_as_name_not_key`
- 预期：命中 `tests/test_view.rs:97`，通过
- 判读：`belongs_to == "客户"`（名字），且 `!= "customer"`（key）
- 类型：功能
- 优先级：**P1**

---

## AC-006 · 冻结后拒绝一切写入 【安全】

### TC-006-01 · 正向：冻结后写入被拒
- 关联验收：AC-006
- 步骤：跑 AC-006 判定命令
- 预期：**无输出**
- 类型：安全
- 优先级：**P0**

### TC-006-02 · 反向：若写入未被拒，守门测试必须 failed
- 关联验收：AC-006
- 步骤：
  1. 临时让 `save_snapshot` 不检查 `frozen_at`
  2. 跑判定命令
  3. 还原
- 预期：输出 **`✗ …`**
- 类型：安全（反向）
- 优先级：**P0**

### TC-006-03 · 判定器自检（陷阱 T-3）：零匹配必须被抓
- 关联验收：AC-006
- 步骤：临时改测试名 → 跑判定命令 → 还原
- 预期：输出 **`✗ …`**
- 类型：安全（判定器自检）
- 优先级：**P0**

### TC-006-04 · 「一切写入」的穷举：不止 `save_snapshot`
- 关联验收：AC-006
- 步骤：对**已冻结版本**逐一尝试下列操作，每条都必须报错（非 0 退出 / `Err`）
  1. `save_snapshot` 写回快照
  2. 以它为 `base` 执行 `proposal apply`
  3. 改其快照内某个对象（新增属性 / 改 `display_name`）
  4. 删除其中的对象类型
- 预期：4 条**全部**被拒
- 判读：只守住 (1) 就算通过 = 「一切」名不副实（见 §3 缺陷 D-08）
- 类型：安全
- 优先级：**P0**

### TC-006-05 · 冻结不可逆
- 关联验收：AC-006（§9.1「冻结是判据」）
- 步骤：
  1. `cargo run -- version --help` —— 确认**无** `unfreeze` / `unfreeze` 类子命令
  2. 尝试对同一版本二次 `freeze`
- 预期：无解冻入口；二次冻结为**幂等成功**或**明确报错**（不接受静默改写 `frozen_at`）
- 类型：安全
- 优先级：**P0**

### TC-006-06 · 端到端交叉验证
- 关联验收：AC-006 × AC-012
- 前置条件：AC-012 全链路已跑通（v1 / v2 已产生）
- 步骤：
  1. `ontok version freeze --version 2`
  2. 对 v2 发起一次写操作
- 预期：第 2 步非 0 退出码，报错信息点名「已冻结 / frozen」且给出版本号
- 类型：安全
- 优先级：**P0**

---

## AC-007 · 关系条目依赖两端已被接受 【红线】

> 判定命令（⚠ filter 与实际测试名不一致，见 TC-007-03）：
> ```bash
> cargo test --tests test_link_acceptance_requires_both_ends_accepted 2>&1 \
>   | grep -qE '^test .* \.\.\. ok' || echo '✗ 该测试不存在或未通过（零匹配也算失败）'
> ```

### TC-007-01 · 正向：两端任一未接受则关系被拦
- 关联验收：AC-007
- 前置条件：Task 6 已完成
- 步骤：
  1. `cargo test --tests test_review_link_acceptance_requires_both_ends_accepted`
  2. 定位：`grep -n 'link_acceptance_requires' tests/test_review_state.rs`
- 预期：第 1 步 `16 passed`（该文件全量）；第 2 步命中 `tests/test_review_state.rs:85`
- 类型：功能
- 优先级：**P0**

### TC-007-02 · 反向：去掉悬空检查必须被抓
- 关联验收：AC-007
- 步骤：
  1. 临时让 `summarize` 不做两端校验（`dangling_relations` 恒空）
  2. `cargo test --tests test_review_link_acceptance_requires_both_ends_accepted`
  3. 还原
- 预期：第 2 步 **FAILED**（该测试 + `test_review_link_missing_both_ends` 同时失败）
- 类型：功能（反向）
- 优先级：**P0**

### TC-007-03 · ⚠ 验收命令与实际测试名一致性核查
- 关联验收：AC-007
- 步骤：`cargo test --tests test_link_acceptance_requires_both_ends_accepted 2>&1 | tail -5`
- 实际观察：实际测试名为 `test_review_link_acceptance_requires_both_ends_accepted`；
  filter `test_link_acceptance_requires_both_ends_accepted` **不是**它的子串 → **零匹配**
- 预期（应为）：`running 0 tests` → 判定命令输出 **✗（假失败）**
- 判读：**AC-007 当前无法通过**。修法：把 AC-007 的 filter 补成
  `test_review_link_acceptance_requires_both_ends_accepted`
  （实施计划「完成标准」§ 里写的就是这个正确名字，是 acceptance.md 漏了 `review_`）。
  修复前判 **AC-007 不通过**。见 §3 缺陷 D-08b
- 类型：功能
- 优先级：**P0**

### TC-007-04 · 两端在基线版本中 → 关系放行
- 关联验收：AC-007
- 步骤：`cargo test --tests test_review_link_ok_when_both_ends_in_baseline`
- 预期：命中 `tests/test_review_state.rs:107`，通过
- 类型：功能
- 优先级：**P0**

### TC-007-05 · 两端在本提案内新增且都已接受 → 关系放行
- 关联验收：AC-007
- 步骤：`cargo test --tests test_review_link_ok_when_both_ends_added_in_same_proposal`
- 预期：命中 `tests/test_review_state.rs:120`，通过（`applicable.len() == 3`）
- 类型：功能
- 优先级：**P0**

### TC-007-06 · 两端都不存在 → 关系被拦
- 关联验收：AC-007
- 步骤：`cargo test --tests test_review_link_missing_both_ends`
- 预期：命中 `tests/test_review_state.rs:133`，通过
- 类型：功能
- 优先级：**P0**

### TC-007-07 · 防御纵深：apply 层也要校验两端
- 关联验收：AC-007 × AC-008
- 步骤：审阅 `src/domain/apply.rs` —— 写入新版本前是否**独立**校验
  `from_type` / `to_type` 存在于 base ∪ 本次 applied
- 预期：**存在**该校验（不依赖 `summarize` 已过滤）
- 判读：仅靠状态机过滤、不在 apply 层复查 → 任何绕过 `summarize` 的调用路径都会产生
  「引用不存在实体的野指针」，正是 AC-007 的**理由**所指
- 类型：功能
- 优先级：**P0**

### TC-007-08 · 未决条目不撑起整体通过
- 关联验收：AC-007（配套）
- 步骤：`cargo test --tests test_review_pending_item_blocks_overall_pass`
- 预期：命中 `tests/test_review_state.rs:163`，通过（`!st.passed` 且已通过条目仍可 apply）
- 判读：此条是 TC-012-05 的判据基础
- 类型：功能
- 优先级：**P0**

---

## AC-008 · 链式 apply 累积而不重置

### TC-008-01 · 正向：两轮链式累积
- 关联验收：AC-008
- 步骤：跑 AC-008 判定命令
- 预期：**无输出**
- 判据：v1 加「客户」→ v2 加「订单」→ **v3 同时含两者**
- 类型：功能
- 优先级：**P0**

### TC-008-02 · 反向：若 apply 用空快照覆盖 base，必须 failed
- 关联验收：AC-008
- 步骤：
  1. 临时让 `apply` 不载入 base 快照，直接以空快照起头
  2. 跑判定命令
  3. 还原
- 预期：输出 **`✗ …`**
- 类型：功能（反向）
- 优先级：**P0**

### TC-008-03 · 判定器自检（陷阱 T-3）：零匹配必须被抓
- 关联验收：AC-008
- 步骤：临时改测试名 → 跑判定命令 → 还原
- 预期：输出 **`✗ …`**
- 类型：功能（判定器自检）
- 优先级：**P0**

### TC-008-04 · 三轮以上仍累积
- 关联验收：AC-008
- 步骤：v1 建版本 → 依次 apply 三个提案（各加一类对象）→ 检查 v4
- 预期：v4 同时含三轮新增的全部对象，且含 v1 原有对象
- 判读：只测两轮会漏掉「第二版正确、第三版起丢」的实现（如每轮只 merge 上一版）
- 类型：功能
- 优先级：**P0**

### TC-008-05 · `seq` 单调递增
- 关联验收：AC-008（计划 Task 8 步骤 4：`new.seq = base.seq + 1`）
- 步骤：连续 3 次 apply 后，查版本列表
- 预期：seq 为 `1,2,3,4`，**不跳号、不重号**；同项目内版本隔离（B 项目不串进 A）
- 类型：功能
- 优先级：**P0**

### TC-008-06 · base 版本不匹配时报错
- 关联验收：AC-008（计划 Task 8 步骤 1：`base_version.seq == proposal.base_version.seq`）
- 步骤：构造一个 base 指向旧版本的提案并 apply
- 预期：报错（非 0 退出 / `Err`），**不产生**新版本
- 判读：若照单全收 → 会用陈旧 base 覆盖他人已应用的变更
- 类型：功能
- 优先级：**P0**

---

## AC-009 · 层级：环检测与悬空父引用

> 判定命令：`cargo test --tests test_hierarchy 2>&1 | grep -qE '^test .* \.\.\. ok' || echo '✗ …'`

### TC-009-01 · 正向：层级套件通过
- 关联验收：AC-009
- 步骤：跑判定命令
- 预期：**无输出**
- 类型：功能
- 优先级：**P0**

### TC-009-02 · 反向：关掉环检测必须 failed
- 关联验收：AC-009
- 步骤：
  1. 临时让 `validate_hierarchy` 不检查环
  2. `cargo test --tests test_hierarchy_detects_cycle`
  3. 还原
- 预期：第 2 步 **FAILED**（`test_hierarchy_detects_cycle` + `…_detects_self_cycle` 同时失败）
- 类型：功能（反向）
- 优先级：**P0**

### TC-009-03 · 判定器自检：`--tests` 依赖测试的物理位置
- 关联验收：AC-009（陷阱 T-1）
- 步骤：
  1. 临时把 `tests/test_hierarchy.rs` 的内容移进 `src/domain/hierarchy.rs` 的 `#[cfg(test)] mod tests`
  2. 跑 `cargo test --tests test_hierarchy`
  3. 还原
- 预期：第 2 步 **零匹配** → 判定命令输出 **✗**
- 判读：AC-009 的判定器**依赖层级测试留在 `tests/` 下**。挪进 lib 会造成**假失败**。
  这是 fail-closed（安全），但需在评审时知晓：**约定：领域纯函数测试一律放 `tests/` 集成测试文件**
- 类型：功能（判定器自检）
- 优先级：**P1**

### TC-009-04 · 环存在时遍历不死循环
- 关联验收：AC-009
- 步骤：`cargo test --tests test_hierarchy_ancestors_stop_on_cycle`
- 预期：命中 `tests/test_hierarchy.rs:35`，通过（`A→B→A` 时 `ancestors_of("A") == ["B"]`）
- 判读：不得超时 / 不得栈溢出
- 类型：功能
- 优先级：**P0**

### TC-009-05 · 悬空父引用被检出
- 关联验收：AC-009
- 步骤：`cargo test --tests test_hierarchy_detects_dangling_parent`
- 预期：命中 `tests/test_hierarchy.rs:111`，通过（问题清单含 `dangling_parent` 前缀项）
- 类型：功能
- 优先级：**P0**

### TC-009-06 · 改挂父前必须预检
- 关联验收：AC-009
- 步骤：`cargo test --tests test_hierarchy_check_reparent_messages`
- 预期：命中 `tests/test_hierarchy.rs:150`，通过
- 判读：三种结果齐备 —— 正常改挂 `Ok`；挂到后代下 `Err(cycle…)`；挂到不存在 `Err(dangling_parent…)`
- 类型：功能
- 优先级：**P0**

### TC-009-07 · 全部 15 个层级测试必须通过（不止前缀匹配）
- 关联验收：AC-009
- 步骤：`cargo test --tests test_hierarchy` → 看结尾汇总行
- 预期：**`15 passed; 0 failed`**（不多不少）
- 判读：AC-009 的判定命令只要求「**至少一行 ok**」，删掉 14 个测试它照样通过。
  本条补上「全量」约束（见 §3 缺陷 D-09）
- 类型：功能
- 优先级：**P0**

---

## AC-010 · 快照往返无损 【红线】

> 判定命令：`cargo test --tests test_snapshot_roundtrip 2>&1 | grep -qE '^test .* \.\.\. ok' || echo '✗ …'`

### TC-010-01 · 正向：save → load 逐字段相等
- 关联验收：AC-010
- 步骤：跑判定命令
- 预期：**无输出**
- 类型：功能
- 优先级：**P0**

### TC-010-02 · 反向：null 语义被吞掉必须 failed
- 关联验收：AC-010
- 步骤：
  1. 临时让 `load` 把 `description: None` 读成 `Some("")`
  2. 跑判定命令
  3. 还原
- 预期：输出 **`✗ …`**
- 类型：功能（反向）
- 优先级：**P0**

### TC-010-03 · 判定器自检（陷阱 T-3）：零匹配必须被抓
- 关联验收：AC-010
- 步骤：临时改测试名 → 跑判定命令 → 还原
- 预期：输出 **`✗ …`**
- 类型：功能（判定器自检）
- 优先级：**P0**

### TC-010-04 · 三态不互吞：`None` / `[]` / `{}`
- 关联验收：AC-010（AC 明确要求「含 `None` / 空列表 / 空 JSON 三态」）
- 步骤：构造一份快照，故意同时含
  ① `description: None` ② `aliases: []` ③ 空 JSON 对象 `{}`（如空的 `effects[].evidence` 或 `key_expr: {}`），
  save → load → 逐项断言
- 预期：往返后 ① 仍 `None` ② 仍 `[]` ③ 仍 `{}`；**不得**三者都变 `null` 或都变空对象
- 判读：这是 serde × sqlx 的 null 语义主坑（AC-010 理由原文所指）
- 类型：功能
- 优先级：**P0**

### TC-010-05 · `extracted_by` 三态往返
- 关联验收：AC-010 × AC-014（计划 Task 4 Step 1 明确要求）
- 步骤：两条往返
  1. `extracted_by: Some(…{version:"2.3.1", round:3, failed_groups:["t_foo"]})` → 存 → 取回**仍为 Some 且各字段一致**
  2. `extracted_by: None` → 存 → 取回**仍为 None**（不是 `Some(默认值)`）
- 预期：两种都无损
- 类型：功能
- 优先级：**P0**

### TC-010-06 · 覆盖写不留残行
- 关联验收：AC-010（计划 Task 4 Step 3：单事务「删旧行 → 写新行」）
- 步骤：同一 version 先存快照 A（3 个对象），再存快照 B（1 个对象），load B
- 预期：load 结果**等于 B**；A 独有的 2 个对象**不残留**；`action_types` / `functions` 的旧行也被清空
- 判读：这是「删旧行」被漏掉的典型表现 —— 往返测试用「只存一次」根本测不到
- 类型：功能
- 优先级：**P0**

### TC-010-07 · 中途失败不留半截快照
- 关联验收：AC-010
- 步骤：令 save 在写入中途失败（如第 2 张表写入时报错），随后 load
- 预期：load 返回**写入前的旧快照**或明确错误；**绝不**返回「3 个对象里只有 1 个」的半截状态
- 判读：非单事务实现的典型症状
- 类型：功能
- 优先级：**P0**

### TC-010-08 · 全空快照可往返
- 关联验收：AC-010
- 步骤：`entity_types` / `link_types` / `action_types` / `functions` 全为空的快照，save → load
- 预期：往返相等；四个集合取回后仍是**空集合**而非 `null`
- 判读：切片 1 的常态就是「action_types 恒空但结构必须在」（AC-003），空集合往返是必经路径
- 类型：功能
- 优先级：**P0**

---

## AC-011 · `extracted_by` 记录 agent 与其具体版本 【红线】

> 判定命令：`cargo test --tests test_snapshot_records_extracted_by 2>&1 | grep -qE '^test .* \.\.\. ok' || echo '✗ …'`

### TC-011-01 · 正向：快照记录 agent 与版本
- 关联验收：AC-011
- 步骤：跑判定命令
- 预期：**无输出**
- 类型：功能
- 优先级：**P0**

### TC-011-02 · 反向：丢失 `extracted_by` 必须 failed
- 关联验收：AC-011
- 步骤：
  1. 临时让 `save_snapshot` 丢弃 `extracted_by`
  2. 跑判定命令
  3. 还原
- 预期：输出 **`✗ …`**
- 类型：功能（反向）
- 优先级：**P0**

### TC-011-03 · 判定器自检（陷阱 T-3）：零匹配必须被抓
- 关联验收：AC-011
- 步骤：临时改测试名 → 跑判定命令 → 还原
- 预期：输出 **`✗ …`**
- 类型：功能（判定器自检）
- 优先级：**P0**

### TC-011-04 · `version` 必须是**具体版本**，不接受 `latest`
- 关联验收：AC-011（AC 预期原文：「能区分同一本体、不同 agent 版本」）
- 步骤：
  1. 写入 `extracted_by.version = "latest"`，save → load，断言取回的不是 `latest`
  2. 审阅 `src/domain/snapshot.rs` / 写路径是否对 `latest` / 空串 / `"*"` 有拒绝或归一化
- 预期：第 1 步**要么被拒（报错），要么被如实保留并由测试判定为违规**；
  不得静默通过并让「具体版本」退化为不可归因
- 判读：AC-011 的核心就是「记具体版本」（设计概要明列的三个刻意决定之一）
- 类型：功能
- 优先级：**P0**

### TC-011-05 · 可区分性：仅 `version` 不同也必须可分辨
- 关联验收：AC-011
- 步骤：构造同一本体的两个版本，`extracted_by` 分别为
  `{agent:"vendor-extractor", version:"1.0.0"}` 与 `{agent:"vendor-extractor", version:"2.0.0"}`
  （**其余字段完全相同**），对比两版快照
- 预期：两版**可被区分**；且 diff 能把「agent 版本变化」作为可归因信息呈现
- 判读：若 diff 忽略 `extracted_by` → 供应商升模型后「diff 变化无法解释」，正是 §8.4 要防的问题
- 类型：功能
- 优先级：**P0**

### TC-011-06 · `round` 必录
- 关联验收：AC-011
- 步骤：写 `round: 3`，往返后断言仍为 `3`
- 预期：往返无损；`round` 不得被默认成 0 或 1
- 类型：功能
- 优先级：**P1**

### TC-011-07 · 与 AC-014 的职责边界
- 关联验收：AC-011 × AC-014
- 步骤：核对测试命名与断言范围
- 预期：
  - **AC-011** 只管「**记不记、记什么粒度**」→ 由 `test_snapshot_records_extracted_by` 覆盖
  - **AC-014** 只管「**取谁的值**（取提案的 / 记 None / 不继承 base）」→ 由 `test_extracted_by_is_not_inherited` 覆盖
  - 两者**不共用**同名测试
- 判读：AC-011 的预期原文「快照能区分同一本体、不同 agent 版本」措辞偏向**取值**，
  与 AC-014 重叠（见 §3 缺陷 D-10）。**修法**：AC-011 判据收敛为「结构与粒度」，
  区分能力由 TC-011-05 单独守
- 类型：功能
- 优先级：**P1**

---

## AC-012 · 端到端可跑（人工）

> 属**人工判据**用例：无法自动化，**但可判定** —— 每条都有明确的退出码 / 输出判据。

### TC-012-01 · 9 条命令全链路成功
- 关联验收：AC-012
- 前置条件：`fixtures/proposal-demo.json` 就位；`ontok` 可执行；PG 可达
- 步骤：依次执行，每条记录退出码
  ```bash
  ontok project new --name acme
  ontok version new  --project acme
  ontok propose import --file fixtures/proposal-demo.json
  ontok review decide --proposal p1 --item 1 --track biz   --accept
  ontok review decide --proposal p1 --item 1 --track model --accept
  ontok proposal apply  --proposal p1
  ontok version freeze --version 2
  ontok ontology show  --version 1
  ontok ontology diff  v1 v2
  ```
- 预期：**9 条全部退出码 0**（逐步 `echo $?` 记录）
- 判读：任一条非 0 → AC-012 **不通过**，记下第几条与 stderr 原文
- 类型：功能（人工）
- 优先级：**P0**

### TC-012-02 · 逐条命令的可观察判据
- 关联验收：AC-012
- 步骤：把 TC-012-01 每条的 stdout 抄进下表并逐条核对
- 预期：

  | # | 命令 | 可观察判据 |
  | --- | --- | --- |
  | 1 | `project new` | 输出新 project id；同名单次运行不报「已存在」以外错误 |
  | 2 | `version new` | 输出 v1，seq=1；`frozen_at` 为空 |
  | 3 | `propose import` | 输出提案 id（记作 `p1`）+ 导入条目数（demo 为 **3** 条） |
  | 4-5 | `review decide` ×2 | 每条回显「item 1 的该轨判定已写入」+ 另一轨状态不变 |
  | 6 | `proposal apply` | 输出新版本 **v2**，seq=2，**未自动冻结**（`frozen_at` 仍空） |
  | 7 | `version freeze` | 回显 v2 冻结时间戳 |
  | 8 | `ontology show --version 1` | 客户视图渲染成功（v1 本体为空，应显示空态而非报错） |
  | 9 | `ontology diff v1 v2` | **输出非空**（至少 1 条 `Added`） |

- 判读：第 8 条若 v1 为空则应显示「（空）」之类的空态；**若崩溃或报「对象不存在」→ 不通过**
- 类型：功能（人工）
- 优先级：**P0**

### TC-012-03 · apply 后的版本状态正确
- 关联验收：AC-012 × AC-006 × AC-008
- 步骤：apply 完成后查 v2
- 预期：v2 存在 · seq=2 · `frozen_at` 为空 · 提案状态变为 `applied`（不自动冻结，计划 Task 8 明列）
- 类型：功能（人工）
- 优先级：**P0**

### TC-012-04 · `diff v1 v2` 非空且可归因
- 关联验收：AC-012 × AC-011
- 步骤：`ontok ontology diff v1 v2`
- 预期：输出至少 1 条 diff 条目；**且 diff 能显示 v2 的 `extracted_by`**（agent + 具体版本 + round）
- 判读：diff 缺 `extracted_by` → agent 换版不可见 → 与 §8.4 冲突
- 类型：功能（人工）
- 优先级：**P0**

### TC-012-05 · ⚠ 命令序列与 AC-007 的一致性核查
- 关联验收：AC-012 × AC-007
- 步骤：
  1. 严格按 AC-012 的命令序列执行（只对 `--item 1` 做了双轨接受）
  2. 检查 v2 是否「含客户与订阅」
- 实际观察：`fixtures/proposal-demo.json` 有 **3 个 item**（① 客户 ② 订阅 ③ 客户—订阅 关系）。
  按 AC-007，② 未接受 → 不可用；③ 依赖两端 → 被拦。按计划 Task 8「逐条应用 applicable item」，
  **v2 只会含「客户」**。
- 预期（按 AC-012 原文）：「apply 后 v2 含客户**与订阅**」—— **无法满足**
- 判读：**这是 acceptance.md 内部的真实矛盾**，见 §3 缺陷 D-11。
  **修法二选一**：
  - 改 AC-012 命令序列：把 item 1 与 item 2 都做双轨接受（再加 2 条 `review decide`）；或
  - 改 AC-012 预期为「v2 含客户；订阅与关系因未接受而不进入 v2」，并把「订阅」验收移至
    「全量接受后」的补充流程
  修复前判 **AC-012 不通过**
- 类型：功能（人工）
- 优先级：**P0**

### TC-012-06 · 可重复执行（外部共享库）
- 关联验收：AC-012（环境约束：外部 PG 多人共用，**不 truncate**）
- 步骤：换一个 project name（如 `acme_tc012_rerun`）把 9 条命令完整重跑一遍
- 预期：第二次全部退出码 0；`acme` 与 `acme_tc012_rerun` 的版本列表**互不可见**
- 判读：跑不通 → 测试间会串数据 / 有全局单例状态
- 类型：功能（人工）
- 优先级：**P1**

### TC-012-07 · 错误路径可读
- 关联验收：AC-012
- 步骤：各执行一次，核对退出码与报错文案
  1. `ontok ontology show --version 999`（版本不存在）
  2. `ontok review decide --proposal 不存在 --item 1 --track biz --accept`
  3. `ontok proposal apply --proposal p1`（对已 applied 的提案重复 apply）
  4. `ontok version freeze --version 2`（对已冻结版本重复冻结）
- 预期：每条**非 0 退出码** + stderr 有具体可读信息（点名参数/状态），**不出现 panic/backtrace**
- 判读：panic 视为不通过 —— FDE 拿到 panic 无法定位
- 类型：功能（人工）
- 优先级：**P1**

---

## AC-013 · 客户视图可读（人工判据）

> 属**人工判据**用例。「可判定」的关键是：**先把判据写成可勾选的表，再让人回答**。

### TC-013-01 · 三名评审人答题，达到 2/3 通过
- 关联验收：AC-013
- 前置条件：
  1. AC-012 已完成
  2. 指定 3 名评审人，**其中不得为本次开发者**
  3. 评审人**未被提前告知**视图内容，也不参与过 `src/domain/view.rs` 的设计
- 步骤：
  1. 执行 `ontok ontology show --version 2`
  2. 3 人**各自独立**回答两问：
     ① 「这些是你们的什么东西？」
     ② 「它有哪几个属性？」
  3. 逐人记录（见 TC-013-02 的表）
- 预期：**至少 2/3 人**同时满足：第 ① 问答出 **且** 第 ② 问答对 **≥ 1 个属性**
- 判定：2 人通过 = **通过**；1 人通过 = **不通过**；0 人通过 = **不通过**
- 类型：功能（人工）
- 优先级：**P0**

### TC-013-02 · 单人答题记录表（判定口径）
- 关联验收：AC-013
- 步骤：每名评审人填一行
- 预期（判定口径必须事先写清，不得事后解释）：

  | 评审人 | ① 答出「是什么」 | ① 原话摘录 | ② 答对属性数 | ② 原话摘录 | 本人是否通过 |
  | --- | --- | --- | --- | --- | --- |
  | R1 | 是/否 | | n/总 | | 是/否 |
  | R2 | 是/否 | | n/总 | | 是/否 |
  | R3 | 是/否 | | n/总 | | 是/否 |

  - 「答出 ①」= 能用自己的话说明这些是**业务上真实存在的东西**（如「客户和他们的订阅」），
    答「数据库的表」也**算答出**（不惩罚用词）
  - 「答对 ②」= 说出的属性名与 `ontology show` 打印的属性**语义一致**（措辞不同算对，
    张冠李戴算错）
  - 记录「原话摘录」而非只记结论 —— 便于复盘投影哪里没做到位
- 类型：功能（人工）
- 优先级：**P0**

### TC-013-03 · 前提防污染校验
- 关联验收：AC-013
- 步骤：答题前逐条确认
- 预期：3 名评审人均满足
  1. 不是本次开发者
  2. 未参与 `src/domain/view.rs` 的设计或评审
  3. 未在答题前看过 `src/domain/view.rs` 或 `tests/test_view.rs`
  4. 独立答题，未互相讨论
- 判读：任一条不满足 → 本次结果作废重测（否则 AC-013 假通过）
- 类型：功能（人工）
- 优先级：**P0**

### TC-013-04 · 人工所见 = 命令所印
- 关联验收：AC-013
- 步骤：把 3 人看到的画面/文本与 `ontok ontology show --version 2` 的原始输出一并留存
- 预期：评审人看到的与命令打印的**完全一致**
- 判读：若 CLI 之外另有「口头讲解版」→ 判据失效
- 类型：功能（人工）
- 优先级：**P1**

### TC-013-05 · 快照内容快照留证
- 关联验收：AC-013 × TC-012-05
- 步骤：答题**前**先把 v2 的实际内容（对象清单 + 属性清单）存证
- 预期：留存的属性清单与 3 人答对计数的分母一致
- 判读：**因 TC-012-05 的矛盾，v2 可能只含「客户」1 个对象**。
  此时第 ② 问的分母随之变化，必须以留证内容为准，不能事后调整分母
- 类型：功能（人工）
- 优先级：**P0**

### TC-013-06 · 不通过时的处理路径留证
- 关联验收：AC-013
- 步骤：若判定为不通过
- 预期：
  1. 记录改的是 `src/domain/view.rs`（**不改测试**——AC-013 明确「不要改测试」）
  2. 改后**重跑** TC-013-01，且**换一批**评审人（原批已见过旧版本，不能复用）
  3. 两次记录都归档
- 判读：改测试来迁就结果 = 直接判 **AC-013 不通过**
- 类型：功能（人工）
- 优先级：**P1**

---

## AC-014 · `extracted_by` 按 ADR-001 取值 【红线】

> 判定命令：`cargo test --tests test_extracted_by_is_not_inherited 2>&1 | grep -qE '^test .* \.\.\. ok' || echo '✗ …'`
> **决策依据**：[ADR-001](../../decisions/ADR-001-extracted-by-policy.md)

### TC-014-01 · 正向：三项断言全部成立
- 关联验收：AC-014
- 步骤：跑判定命令
- 预期：**无输出**
- 判据：① 提案带值 → 新版本记该值 ② 提案不带 → 新版本记 `None` ③ **新版本绝不继承 base**
- 类型：功能 / 架构
- 优先级：**P0**

### TC-014-02 · 反向（核心）：让 apply 继承 base 的值，必须 failed
- 关联验收：AC-014
- 步骤：
  1. 临时把 `apply` 改为「提案无值时回退取 base 的 `extracted_by`」
  2. 跑判定命令
  3. 还原
- 预期：输出 **`✗ …`**
- 判读：这是 ADR-001 明确**否掉的方案**（「一律继承」），也是本 AC 的核心红线。
  判定器抓不住 → AC-014 **不通过**
- 类型：架构（反向）
- 优先级：**P0**

### TC-014-03 · 判定器自检（陷阱 T-3）：零匹配必须被抓
- 关联验收：AC-014
- 步骤：临时改测试名 → 跑判定命令 → 还原
- 预期：输出 **`✗ …`**
- 类型：架构（判定器自检）
- 优先级：**P0**

### TC-014-04 · 场景 B：新版本原样取提案的 agent 版本
- 关联验收：AC-014
- 步骤：base v1（`extracted_by = A/1.0.0`）→ 提案带 `A/2.4.0` → apply → 查 v2
- 预期：v2 `extracted_by == A/2.4.0`；**恰好等于**提案的值
- 类型：功能
- 优先级：**P0**

### TC-014-05 · 场景 A：提案不带值 → 新版本记 `None`
- 关联验收：AC-014
- 步骤：base v1（`extracted_by = A/1.0.0`）→ 提案**不带** `extracted_by` → apply → 查 v2
- 预期：v2 `extracted_by is None`
- 判读：**不是** `A/1.0.0`（继承）、**不是**「按场景推断出 A」
- 类型：功能
- 优先级：**P0**

### TC-014-06 · 不推断、不分支
- 关联验收：AC-014（ADR-001「不猜是本决策的核心」）
- 步骤：提案带 `extracted_by.agent = null`（混合提案中有人工成分）→ apply → 查 v2
- 预期：**如实记录**，不得补全 agent 名、不得回退到 base 的 agent
- 判读：出现任何「智能推断」→ 违反 ADR-001
- 类型：功能
- 优先级：**P0**

### TC-014-07 · 链式易错点：`None` 也不能当继承源
- 关联验收：AC-014
- 步骤：
  1. v1 = A/1.0.0
  2. 提案 P2 不带值 → **v2 = None**
  3. 基于 v2 的提案 P3 带 B/2.0.0 → **v3 = B/2.0.0**
- 预期：第 3 步 v3 = `B/2.0.0`
- 判读：**易错实现**：「若 base 为 `None` 则用默认值 / 沿用上一非空值」→ v3 会拿到 `A/1.0.0`。
  这条专门守该实现缺陷
- 类型：功能
- 优先级：**P0**

---

## AC-015 · `failed_groups` 不继承 【红线】

> 判定命令：`cargo test --tests test_failed_groups_not_inherited 2>&1 | grep -qE '^test .* \.\.\. ok' || echo '✗ …'`

### TC-015-01 · 正向：每轮独立记录
- 关联验收：AC-015
- 步骤：跑判定命令
- 预期：**无输出**
- 类型：功能
- 优先级：**P0**

### TC-015-02 · 反向：让 `failed_groups` 继承 base，必须 failed
- 关联验收：AC-015
- 步骤：
  1. 临时让 `apply` 把 base 的 `failed_groups` 并入新版本
  2. 跑判定命令
  3. 还原
- 预期：输出 **`✗ …`**
- 类型：功能（反向）
- 优先级：**P0**

### TC-015-03 · 判定器自检（陷阱 T-3）：零匹配必须被抓
- 关联验收：AC-015
- 步骤：临时改测试名 → 跑判定命令 → 还原
- 预期：输出 **`✗ …`**
- 类型：功能（判定器自检）
- 优先级：**P0**

### TC-015-04 · 空列表语义：不是继承，也不是 `None`
- 关联验收：AC-015
- 步骤：v1 的 `failed_groups = ["t_foo"]` → 提案不带 `extracted_by` → apply → 查 v2
- 预期：v2 的 `failed_groups` 为 **`[]`（空列表）**
- 判读：得到 `["t_foo"]` = 继承（违规）；得到 `None` = 类型建模与 ADR-001 后果 1 冲突
  （`extracted_by` 为 `None` 时 `failed_groups` 根本无载体）—— 三者只能取其一
- 类型：功能
- 优先级：**P0**

### TC-015-05 · 不参与收敛判定
- 关联验收：AC-015（AC 理由原文：「继承会让上轮失败的组被误认为本轮已覆盖 → 静默失败伪造收敛」）
- 步骤：
  1. v1 抽取失败组 `t_foo`
  2. v2 抽取**再次**失败 `t_foo`
  3. 检查「是否已收敛」的判定逻辑
- 预期：v2 再次失败 `t_foo` 时**不得**被判为收敛 / 不得因「v1 已覆盖过 t_foo」而跳过该组
- 类型：功能
- 优先级：**P0**

### TC-015-06 · 来回失败可追溯
- 关联验收：AC-015
- 步骤：v1 失败 `t_foo` → v2 成功（`[]`）→ v3 又失败 `t_foo`，逐版查 `failed_groups`
- 预期：v1=`["t_foo"]` · v2=`[]` · v3=`["t_foo"]`，**三版各自独立**
- 判读：任一版本被前序污染 → 失败历史不可解释，本 AC 失效
- 类型：功能
- 优先级：**P0**

---

## 覆盖对照表

| AC | 名称 | 类型 | TC 编号 | 条数 | 最高优先级 | 已有实现测试 |
| --- | --- | --- | --- | --- | --- | --- |
| **AC-001** | 测试全绿 | 功能 | TC-001-01 ~ 03 | 3 | P0 | — |
| **AC-002** | 领域层零 I/O | 架构·红线 | TC-002-01 ~ 06 | 6 | P0 | — |
| **AC-003** | 快照契约含全部占位 | 架构·红线 | TC-003-01 ~ 06 | 6 | P0 | — |
| **AC-004** | 切片 1 无写回路径 | 安全·红线 | TC-004-01 ~ 06 | 6 | P0 | — |
| **AC-005** | 客户视图零建模术语 | 功能·红线 | TC-005-01 ~ 08 | 8 | P0 | `tests/test_view.rs`（11 个） |
| **AC-006** | 冻结后拒绝一切写入 | 安全 | TC-006-01 ~ 06 | 6 | P0 | — |
| **AC-007** | 关系依赖两端被接受 | 功能·红线 | TC-007-01 ~ 08 | 8 | P0 | `tests/test_review_state.rs`（16 个） |
| **AC-008** | 链式 apply 累积 | 功能 | TC-008-01 ~ 06 | 6 | P0 | — |
| **AC-009** | 环检测与悬空父 | 功能 | TC-009-01 ~ 07 | 7 | P0 | `tests/test_hierarchy.rs`（15 个） |
| **AC-010** | 快照往返无损 | 功能·红线 | TC-010-01 ~ 08 | 8 | P0 | — |
| **AC-011** | 记录 agent 与具体版本 | 功能·红线 | TC-011-01 ~ 07 | 7 | P0 | — |
| **AC-012** | 端到端可跑 | 功能·人工 | TC-012-01 ~ 07 | 7 | P0 | — |
| **AC-013** | 客户视图可读 | 功能·人工 | TC-013-01 ~ 06 | 6 | P0 | — |
| **AC-014** | `extracted_by` 按 ADR-001 | 功能/架构·红线 | TC-014-01 ~ 07 | 7 | P0 | — |
| **AC-015** | `failed_groups` 不继承 | 功能·红线 | TC-015-01 ~ 06 | 6 | P0 | — |
| — | **合计** | | | **97** | | 已有 42 |

### 红线类 AC 的反向测试覆盖核查

| 红线 AC | 正向 | 反向（故意违规） | 判定器自检 | ≥3 条 |
| --- | --- | --- | --- | --- |
| AC-002 | TC-002-01 · 06 | TC-002-02 · 04 | TC-002-03 · 05 | ✅ 6 |
| AC-003 | TC-003-01 · 06 | TC-003-02 · 04 | TC-003-03 | ✅ 6 |
| AC-004 | TC-004-01 · 05 · 06 | TC-004-04 | TC-004-02 · 03 | ✅ 6 |
| AC-005 | TC-005-01 · 06 · 08 | TC-005-02 | TC-005-03 · 04 · 05 | ✅ 8 |
| AC-007 | TC-007-01 · 04~06 · 08 | TC-007-02 · 07 | TC-007-03 | ✅ 8 |
| AC-010 | TC-010-01 · 04~08 | TC-010-02 · 06 · 07 | TC-010-03 | ✅ 8 |
| AC-011 | TC-011-01 · 04~07 | TC-011-02 · 05 | TC-011-03 · 07 | ✅ 7 |
| AC-014 | TC-014-01 · 04~07 | TC-014-02 · 05 · 06 | TC-014-03 | ✅ 7 |
| AC-015 | TC-015-01 · 04~06 | TC-015-02 · 05 | TC-015-03 | ✅ 6 |

> **判定器自检**= 验证「验收命令本身能抓到违规」；AC-002 / AC-005 这类靠 shell 判定的 AC 必含。

### 已有实现测试的复用对照

| 已有测试文件 | 测试数 | 被哪些 TC 引用 |
| --- | --- | --- |
| `feature-REQ-002/tests/test_hierarchy.rs` | 15 | TC-009-01 · 02 · 04 · 05 · 06 · 07 |
| `feature-REQ-002/tests/test_review_state.rs` | 16 | TC-007-01 · 02 · 04 · 05 · 06 · 08 |
| `feature-REQ-002/tests/test_view.rs` | 11 | TC-005-01 · 02 · 04 · 06 · 08 |

> 其余 12 条 AC（AC-001/002/003/004/006/008/010/011/012/013/014/015）**尚无实现测试**，
> 对应 Task 1~5 · 8 · 10 待补。

---

## §3 · 验收标准自查：缺陷与矛盾清单

> 本节是 QA 在拆解 15 条 AC 时**实际发现**的问题，按严重度排序。
> 标注「**阻断**」= 修好之前该 AC 无法判为通过。

### 🔴 阻断级

#### D-07 · AC-005 的验收命令与实际测试名不匹配 → **假失败**
- **位置**：`acceptance.md:56`
- **问题**：filter 写 `test_biz_view_never_exposes_modeling_terms`，
  实际测试名为 `test_view_biz_view_never_exposes_modeling_terms`（`tests/test_view.rs:69`）。
  cargo 的 filter 是**子串匹配**，`test_biz_view_…` **不是** `test_view_biz_view_…` 的子串 → 零匹配。
- **表现**：AC-005 判定命令必然输出 `✗`，无论实现对错。
- **修法**：filter 补成 `test_view_biz_view_never_exposes_modeling_terms`（推荐，贴合分组前缀约定）。
- **跟踪**：TC-005-05

#### D-08b · AC-007 的验收命令同样与实际测试名不匹配 → **假失败**
- **位置**：`acceptance.md:81`
- **问题**：filter 写 `test_link_acceptance_requires_both_ends_accepted`，
  实际为 `test_review_link_acceptance_requires_both_ends_accepted`（`tests/test_review_state.rs:85`）。
- **旁证**：实施计划「完成标准」§ 写的是**正确**名字（`…test_review_link_acceptance…`），
  是 `acceptance.md` 漏了 `review_` 段。两处文档互相矛盾。
- **修法**：AC-007 的 filter 补 `review_`。
- **跟踪**：TC-007-03

#### D-11 · AC-012 的命令序列与 AC-007 互相矛盾 → **AC-012 无法满足自己的预期**
- **位置**：`acceptance.md:143-154`
- **问题**：AC-012 只对 `--item 1` 做了双轨接受，但 `fixtures/proposal-demo.json` 有 **3 个 item**
  （① 客户 ② 订阅 ③ 客户—订阅 关系）。按 AC-007，② 未接受 → 不可用；③ 依赖两端 → 被拦。
  按计划 Task 8「逐条应用 applicable item」，**v2 只会含「客户」**。
- **矛盾**：AC-012 的预期写「apply 后 v2 含客户**与订阅**」—— **与 AC-007 不相容**。
- **修法二选一**：
  - 改命令序列：给 item 1、item 2 都做双轨接受（多 2 条 `review decide`）；
  - 或改预期为「v2 含客户；订阅与关系因未接受而不进入 v2」，把订阅验收移到「全量接受后」的补充流程。
- **连带影响**：AC-013 依赖 AC-012，且第 ② 问「它有哪几个属性」的分母随 v2 内容变化。
- **跟踪**：TC-012-05 · TC-013-05

### 🟠 判定器缺陷（会造成假通过）

#### D-02 · AC-002 的 `grep -vE '\s*//'` 会把「代码 + 行尾注释」整行剔除 → **假通过**
- **位置**：`acceptance.md:19-20`
- **问题**：过滤的是**整行**。一行 `let _s = crate::db::Store; // 暂时借用` 含 `db::`，
  却因含 `//` 被整行剔除 → 判定为 `✓`。
- **修法**：先剥掉行尾注释再匹配，例如
  `grep -rn 'sqlx\|PgStore\|db::' src/domain/ | sed -E 's@//.*@@' | grep -n 'sqlx\|PgStore\|db::' && echo '✗ 违规' || echo '✓'`
- **跟踪**：TC-002-04

#### D-03 · `src/domain/` 不存在时，AC-002 判定输出 `✓` → **假通过**
- **位置**：`acceptance.md:19-20`
- **问题**：`grep -rn` 对不存在的目录退出码 2，被 `|| echo '✓'` 吞掉。
  目录被整体删掉/改名，架构红线照样「通过」。
- **修法**：前置 `test -d src/domain || { echo '✗ 目录缺失'; exit 1; }`
- **跟踪**：TC-002-05

#### D-04 · AC-003 未排除注释行（与 AC-002 做法不一致）→ **假通过**
- **位置**：`acceptance.md:30-32`
- **问题**：AC-002 用 `grep -vE '\s*//'` 排除注释，AC-003 **没有**。
  在 `snapshot.rs` 里写一行 `// functions 属于切片 2` 就能满足 `grep -q functions`。
- **修法**：AC-003 的循环内同样加注释排除，或统一封装成 `scripts/check-domain.sh`。
- **跟踪**：TC-003-03

#### D-05 · AC-003 清单漏掉 `WriteBinding` 内部 4 个子字段
- **位置**：`acceptance.md:30` vs 设计概要「占位项（红线）」
- **问题**：清单只查 `WriteBinding` **类型名**。规格 §5.2 要求的
  `key_expr` / `value_map` / `supported_values` / `conflict_field` 被后人清理时**不会**被抓
  —— 而「只建不用容易被后人清理」正是 AC-003 存在的理由。
- **修法**：清单补 4 项。
- **跟踪**：TC-003-04

#### D-06 · AC-004~AC-011 的判定器只验「测试存在且 ok」，验不出空壳测试
- **位置**：`acceptance.md` 各条
- **问题**：`assert!(true)` 也能让判定命令「通过」。这类「测试存在性」判定对
  **测试质量**零约束，只能防「测试被删」。
- **修法**：不改判定命令；改为在评审清单里加「守门测试必须有实质断言」这条人工检查
  （已落到 TC-004-04）。
- **跟踪**：TC-004-04

#### D-09 · AC-009 的判定只要求「至少一行 ok」，不保证 15 个全过
- **位置**：`acceptance.md:106-108`
- **问题**：删掉 14 个层级测试只留 1 个，AC-009 照样通过。
- **修法**：核对结尾汇总行 `15 passed; 0 failed`。
- **跟踪**：TC-009-07

### 🟡 文档一致性

#### D-01 · AC-001 前置条件与实施计划的测试库方案矛盾
- **位置**：`acceptance.md:9` vs 实施计划 §环境
- **问题**：AC-001 写「Docker（供 testcontainers 起真 PG）」；
  实施计划明确「**不用 testcontainers** —— 本机 Docker daemon 未起，且外部库更省事」，
  用外部 PG 16.14 + `ONTOK_TEST_DATABASE_URL`。
- **修法**：AC-001 前置条件改为「Rust 1.86+、外部 PostgreSQL 16.14 可达（`ONTOK_TEST_DATABASE_URL`）」。
- **跟踪**：TC-001-02

#### D-05b · AC-003 的检查项在两份文档里拼写不一致
- **位置**：`acceptance.md:30`（`WriteBinding`）vs 实施计划「完成标准」§（`write_bindings`）
- **问题**：`grep -q "write_bindings"` 匹配不到 `WriteBinding`（下划线 vs 驼峰）→ 实施计划那版必假失败。
- **修法**：两处统一为 `WriteBinding`。
- **跟踪**：TC-003-01

#### D-12 · 实施计划说「8 条命令」，AC-012 实际列 9 条
- **位置**：实施计划 Task 10 Step 3 vs `acceptance.md:154`
- **问题**：AC-012 列了 9 行命令且预期写「9 条命令全部成功」；实施计划写「跑通 AC-012 那 8 条命令」。
- **修法**：实施计划改为 9 条。
- **跟踪**：TC-012-01

#### D-08 · AC-006「一切写入」未被枚举
- **位置**：`acceptance.md:64-74`
- **问题**：判定命令只验 `test_frozen_version_rejects_writes` 一个测试，
  很可能只覆盖 `save_snapshot`。但「**一切**写入」至少还包括：以冻结版本为 base 的
  `proposal apply`、改快照内容、删对象类型。
- **修法**：在 `tests/test_apply.rs` 里把上述入口逐一纳入同名守门测试。
- **跟踪**：TC-006-04

#### D-10 · AC-011 与 AC-014 职责边界模糊
- **位置**：`acceptance.md:126-137` vs `acceptance.md:191-207`
- **问题**：AC-011 的预期「快照能区分同一本体、不同 agent 版本」说的是**取值/区分能力**，
  而取值规则由 ADR-001 / AC-014 规定。两条 AC 的测试范围会重叠，且 AC-011 的
  `test_snapshot_records_extracted_by` 与 AC-014 的 `test_extracted_by_is_not_inherited` 命名相近、易混。
- **修法**：把 AC-011 收敛为「**结构与粒度**」（有没有记、记不记具体版本、round 在不在），
  「区分能力」单列一条用例；AC-014 独占「**取谁的值**」。
- **跟踪**：TC-011-05 · TC-011-07

### ⚪ 不可测试 / 不可完备证明

#### U-01 · AC-004「无**任何**可执行写回路径」在字面上无法完备证明
- **说明**：单个测试只能证伪它想到的路径，无法穷尽「任何路径」。
  实务上能做到的是「已知写回入口清单 + 逐条断言不存在」。
- **处置**：TC-004-05 / TC-004-06 给出该清单的兜底核查。
  **判定口径**：「清单内 0 个可执行入口」= 通过；不声称数学意义上的完备。

#### U-02 · AC-013 依赖「3 名评审人」这一外部条件
- **说明**：在没有真实客户方评审人的环境里，AC-013 **无法执行**，只能标「阻塞」。
  退而求其次的替代（同事代读）**不能**代表客户可读性，会造成假通过。
- **处置**：TC-013-03 强制校验评审人资质；若拿不到合规评审人，AC-013 应判为
  **「未执行（阻塞）」而非「通过」**。

#### U-03 · AC-009 的判定器依赖测试的**物理位置**（`tests/` 而非 `src/` 内联 `#[cfg(test)]`）
- **说明**：`cargo test --tests <filter>` 只扫 `tests/` 下的集成测试。
  把层级测试挪进 `src/domain/hierarchy.rs` 的 `#[cfg(test)] mod tests` 会**零匹配** → 假失败。
- **处置**：约定「领域纯函数测试一律放 `tests/` 集成测试文件」，并在 TC-009-03 把该行为显式记录。

#### U-04 · AC-012 无法完全自动化
- **说明**：AC-012 是 CLI 手工序列（会改共享测试库状态），自动化需先解决库状态隔离
  （项目已定「不 truncate」）。
- **处置**：保持人工执行，但 TC-012-01/02 已把「退出码 + 可观察输出」写成可勾选判据，
  满足「可判定」要求。

---

## §4 · 建议的最小修复清单（改 `acceptance.md`）

> 按优先级排列。**前 3 条不修，AC-005 / AC-007 / AC-012 就无法判定。**

| # | 缺陷 | 修法 | 位置 |
| --- | --- | --- | --- |
| 1 | **D-07** | AC-005 filter → `test_view_biz_view_never_exposes_modeling_terms` | `acceptance.md:56` |
| 2 | **D-08b** | AC-007 filter → `test_review_link_acceptance_requires_both_ends_accepted` | `acceptance.md:81` |
| 3 | **D-11** | AC-012 补 item 2 的双轨接受，或改预期 | `acceptance.md:143-154` |
| 4 | **D-01** | AC-001 前置条件改「外部 PG + `ONTOK_TEST_DATABASE_URL`」 | `acceptance.md:9` |
| 5 | **D-02** | AC-002 grep 改为「剥行尾注释后再匹配」 | `acceptance.md:19-20` |
| 6 | **D-03** | AC-002 前置加 `test -d src/domain` | `acceptance.md:19-20` |
| 7 | **D-04** | AC-003 循环内加注释排除 | `acceptance.md:30-32` |
| 8 | **D-05** | AC-003 清单补 `key_expr` / `value_map` / `supported_values` / `conflict_field` | `acceptance.md:30` |
| 9 | **D-05b** | 实施计划「完成标准」的 `write_bindings` → `WriteBinding` | 实施计划 §完成标准 |
| 10 | **D-12** | 实施计划「8 条命令」→ 9 条 | 实施计划 Task 10 Step 3 |
| 11 | **D-08** | AC-006 守门测试纳入全部写入口 | `acceptance.md:69` |
| 12 | **D-10** | AC-011 判据收敛为「结构与粒度」 | `acceptance.md:135` |
