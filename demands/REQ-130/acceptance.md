# 验收标准：REQ-130

## 一 · AC

### AC-001 · N-3 判据改成按条目匹配

- 新判据断言：`merged.json` 里**不存在** `name == <被排除概念>` 的 concept 条目、**不存在** relation 条目的任一端**等于**它、**不存在** `semantic`（谓词）**等于**它的 relation 条目
- ⛔ 不再用整文件 grep
- 判据：真实 fixture 上跑，**绿**；同时 `excluded_not_alignable == 28` 仍成立

### AC-002 · N-3 的反向判据（防"改松了"）

- 手工往 `merged.json` 塞一条 `alignable:false` 的**条目** ⇒ 新判据**必须红**
- 判据：这是"判据有牙"的唯一证明

### AC-003 · N-2 字段

- 给 `--at` ⇒ 产物含 `signed_at_source: "arg"`
- 省略 `--at` ⇒ 产物含 `signed_at_source: "system"`
- 判据：两次各一，字段值正确

### AC-004 · 确定性与兼容

- 同参数（含 `--at`）两次 sign ⇒ 逐字节相同
- 读回**缺少** `signed_at_source` 的老产物 ⇒ 不报错（按缺省处理）
- 判据：各一次

### AC-005 · 不退化

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` + `doc merge/sign` 的其他行为 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言（**这次是收紧判据，不是放宽**）、新增依赖 |
| 基线 | 同机 `develop` 实测 |

## 二 · 纪律

- **G** 变异：把新判据改回 grep ⇒ AC-002 必须红
- **S** AC 也会错，发现直接说


---

## 三 · Dev 交付与裁定（R-1 / R-2）

### R-1 · 老产物缺字段的缺省值：**用 `legacy`，不用 `unknown`**

Dev 用了 `legacy`（demand 风险表里我写的是 `system`/`unknown`），并说明理由。

**裁定：按 Dev 的 `legacy`。**

| 值 | 含义 | 判断 |
| --- | --- | --- |
| `arg` | `--at` 给的（**声明**） | ✅ |
| `system` | 系统时钟取的（**记录**） | ✅ |
| `unknown` | 不知道 | 可用，但没说明"为什么不知道" |
| **`legacy`** | **老产物没有这一栏** | ✅ **更好**：它回答了"为什么不知道" |

**理由**：写 `system` 是**伪造审计结论**（老产物里 arg/system 已不可分）；补写是**改写历史**。
`legacy` 既诚实又带信息。

**要求**：`--help` / 模块头里把三个值列全（`arg | system | legacy`），别只写两个。

### R-2 · AC-001 的 relation 身份**多算了 `semantic`**：接受（收紧，不是放宽）

Dev 说：relation 判据把 `payload.semantic`（谓词）也算进身份，比 AC-001 字面「subject/object」更严；
理由是被排除的关系是「订单 -[退款]-> 客户」，只查两端的话「退款」那半条会**空转**。

**裁定：接受。** 这是收紧，且理由成立（否则判据在那条用例上是空的）。

**但要求**：把 AC-001 的措辞改成「concept 判 name/target；relation 判两端 **+ 谓词（semantic）**」，让文档与实现一致。

### R-3 · 变异证据（Dev 已做，QA 复核）

| 变异 | 红在哪 |
| --- | --- |
| 判据改回整文件 grep | 反向判据 + 真实 fixture 双红：`引文里提到「积分转赠」≠ 条目被并入` |
| `signed_at_source` 恒 `arg` | `system` 与「arg≠system 可分」两条红 |
| 去掉老产物兼容 | `ac004_legacy` 红（panic 在 `doc_merge.rs:1872`） |


### R-4 · QA 的 4 条非阻断

| # | 项 | 处置 |
| --- | --- | --- |
| **N-1** | `doc sign --help` 只写 `arg`/`system`，没列 `legacy` | **必修**（三值必须一致） |
| **N-2** | relation 判据用 `contains` ⇒ 合法边 `套餐退款→客户` 会**假红** | **必修**：两端与 `semantic` 改**相等**，并要求**绿红成对**验证 |
| N-3 | `missing(...).unwrap()` 把「fixture 缺失」变成 panic 而非 SKIP | 登记；下轮统一改「响亮 SKIP」（至少不是假绿） |
| D-1 | AC-001 字面未同步 `semantic` | ✅ 本文件已改（见上） |

**N-2 为什么必修**：这个需求存在的全部理由就是「判据不许假红」。用 `contains` 只是把假阳性从 `evidence.quote` 搬到「端点/谓词包含关系」，**性质一样**。

### 方法学提醒（QA 提的，记进规程）

> **共享 `target/` 做变异会导致缓存假象**（QA 遇到一次，`cargo clean -p ontok` 重建后才对）。
> ⇒ 变异 worktree 应给**独立 `CARGO_TARGET_DIR`**。
