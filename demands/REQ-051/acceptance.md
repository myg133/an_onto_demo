# 验收标准：REQ-051 真实 adapter 的 `attempt` 编号覆盖 + 编排层碰撞检测

> 依据 REQ-050 验收（QA 判 PASS 带非阻断，F1 中等）写成。基线：`f43940f`（develop）。
>
> **这是 REQ-050 唯一复发路径的封堵。** 不补它，REQ-050 的修复随时可被一行绕过。

---

## 背景：为什么这不是「判据不够强」而是「判据根本不存在」

REQ-050 新增的 7 条 AC 测试用的是**替身** `RetryingUsageAgent`（自己重写编号逻辑），
`test_acp_protocol.rs` 直接把 `0` 当字面量传给 `delta_to_report`。

⇒ **两个真实 adapter（`LlmAgent` / `AcpAgent`）的 `attempt` 编号逻辑零覆盖。**

| 变异 | 结果 |
|---|---|
| **M3** `LlmAgent` 恒报 `attempt: 0` | **存活** —— 9 套件 **168 全绿** |
| **M3b** `AcpAgent` 恒报 `0` | **存活** |
| **M4** ACP 编号错位（+1） | **存活** |

**这正是 REQ-050 登记的唯一复发路径**：新 adapter 忘编 `attempt` ⇒ 静默退回少计，
而**没有任何测试会响**。

---

## AC-001 · 真实 adapter 的编号逻辑被覆盖

- 预期结果：存在测试**走真实 `LlmAgent` / `AcpAgent`**，验证同组重试时
  `attempt` 递增（0、1、…）且上报的 `used` **逐笔进账**
- 类型：测试有效性
- **判别力自证（必做）**：
  - **M3**（删 `LlmAgent` 编号）⇒ **必须变红**
  - **M3b**（删 `AcpAgent` 编号）⇒ **必须变红**
  - **M4**（ACP 编号错位 +1）⇒ **必须变红**
  - 每条给**二进制名 + 运行条数**
- ⚠️ **编译失败不算变异执行**

## AC-002 · 编排层碰撞检测（QA 已实测可行）

**QA 实测结论**：
- 编排层做「同 `(group_id, attempt)` 撞出**不同** `used`/`cost` 就报错」
- **168 条测试零假阳性**（快照重读的同键同值**不**误报 —— 那正是合法情形）
- `catch_unwind` 探针确认**真能抓到**坏 adapter

> ⚠️ **这推翻了 Dev「无法被代码强制」的说法** —— 他说的是「他范围内无法强制」，
> 但作为整体判断偏保守。**本条要求落地。**

- 预期结果：碰撞时**报错而非静默**，且**不误报**合法情形
- 类型：架构
- ⚠️ **零假阳性是硬要求**。若检测逻辑在**正常路径**上误报，**宁可不做** ——
  一次误报就会让所有重试场景变红，比漏检更糟

## AC-003 · 碰撞检测的边界

- 预期结果：检测只在「**同一 `(group_id, attempt)` 对应不同用量**」时触发。
  **同键同值**（快照重读）**必须放行** —— 那是既有 `#dup` 场景的正常形态
- 类型：功能
- **判别力自证**：
  - 制造真碰撞（构造两个不同用量的同键上报）⇒ **必须触发**
  - 制造合法同键同值（重读同一快照）⇒ **必须不触发**
  - 两条各给**二进制名 + 运行条数**

---

## AC-004 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**566 passed / 0 failed / 1 ignored**（REQ-050 之后的基线）
  + 本需求新增用例数
- **建议同会话基线对照**（基线 `f43940f` = **566/0/1**）；耗时**别声称精度**
- 红线：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs` 零改动；
  `test_humanize_still_refuses_unknown_columns`（`src/domain/view.rs:1254`）一条断言都没放宽；
  **无测试被删除或 `#[ignore]`**
  （⚠️ `tests/test_agent_rebuild.rs:337` 的死代码 `let report` 是**基线已有**，
  不属本需求，可不动）
- 产物 sha **不变**（本需求不碰抽取逻辑）：
  - `stats.json` = `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba`
  - `proposal.json`（`--project extracted`）= `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635`
  - **输入就是 `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`**
    （**BA 已多次实测：直接用它当 `--schema`，两个 sha 全部精确 MATCH**）
  - 命令实属 **`ontok-propose`**；`--out` 在 `--stats` 之前；**不传 `--names`**

---

## ⚠️ 环境事实（上一位 Dev 在这里栽过）

- `.env` **在仓库根**（`/home/myg133/Projects/Anchnet/Ontology/.env`），各 worktree 内**没有**。
  库名是 `notol`，**必须替换成 `ontol`**（该库名在实例上根本不存在）
- 正确取值：
  ```bash
  export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' \
    /home/myg133/Projects/Anchnet/Ontology/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"
  ```
- **`relative URL without a base` = URL 取残缺，不是环境坏了** ——
  上一位 Dev 因此报了 121 failed「环境不可达」，那是他的取值错误
- 本机**没有 `psql`、没有 `psycopg`** ⇒ **测不了 `SELECT 1` p50**
- 共享本体库**只读**；**禁 DELETE / TRUNCATE / 清空表**
- 报告把「代码失败」与「环境类失败」**分开写**；复现率随负载浮动，**不声称稳定值**
- 口令只走环境变量，**绝不写进代码、文件、日志、报告或 commit**；
  ⚠️ **不要 `echo` 连接串**

## 红线清单

| 文件 / 约束 | 说明 |
| --- | --- |
| `tests/drop_rate.rs` | **禁改** |
| `tests/test_fk_naming.rs` | **禁改** |
| `src/domain/view.rs` | **禁改** |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| 任何测试 | **不许删除、不许 `#[ignore]`** |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖** |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target（本会话连踩两次）
- ⚠️ **不要与另一个全量测试并发跑**（本会话已两次互相拖垮）
- **求和不要用 `paste -sd+`**（逐行输出 ⇒ 内容重复即精确翻倍）
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- 临时文件只放 `.review-workspace/req051/`；**禁 `/tmp`、禁 home**
  （QA 上轮自曝探针**必须放 `tests/`** 才能编译，且**必须收尾删干净**）
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 核对文件用**裸 `grep`**。禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
