# 验收标准：REQ-129 · 文档断言合并进本体提案

> **一句话**：把文档断言**机械地**并进提案，语义对齐留给人；**未签的绝不进快照**。

## 一 · AC

### AC-001 · 命令可用 + 确定性

- `ontok doc merge --assertions A --proposal P --out M` ⇒ 退出码 0，产出 `M`
- 同输入两次运行 ⇒ **`M` 逐字节相同**（sha256 相等）
- 判据：两次 sha256 + `--stats` 落盘

### AC-002 · 只并 `alignable: true`

- 输入含 `modality: future` / `example`（`alignable: false`）的断言 ⇒ **不并入**
- 判据：`stats.excluded_not_alignable` 计数与输入中该类条数相等；`M` 里 grep 不到这些名字

### AC-003 · doc 条目最高 medium、必须未签

- 所有 `origin: "doc"` 或 `origin: "both"` 的条目：
  - `band` ∈ {`low`, `medium`}（**不含 `strong`**）
  - `reviewed_by` 为 `null`
- 判据：逐条断言；变异（把 doc 条目设成 strong）⇒ 必须有测试红

### AC-004 · 无映射的关系进 unmapped，不发明表名

- 关系断言没有 `maps_to` ⇒ 进 `doc_unmapped`，`M` 里不出现对应关系边
- 判据：`stats.doc_unmapped` 计数正确；`M` 中所有 `maps_to` 的表/列**都存在于 schema.json**
- 变异：让合并器按名字猜一张表 ⇒ 必须有测试红（猜错时映射不存在）

### AC-005 · 同实体合并为 `both`

- doc 概念与 DB 实体映射到同一张表 ⇒ 一条条目，`origin: "both"`，**两侧证据都在**
- 判据：证据条数 = 两侧之和（去重后）；⛔ 不许出现两条指向同一表的实体

### AC-006 · 冲突双保留且不可 apply

- doc 说「一个订单一个套餐」、DB 推断 1:N ⇒ 生成 `conflicts[]` 条目：
  - `doc_says` / `db_says` 都在
  - `applicable: false`
- 判据：applier 拿到 `applicable: false` 的条目 ⇒ **拒绝**（不是忽略），并点名
- 变异：把 `applicable` 去掉 ⇒ 必须有测试红

### AC-007 · 端到端（**最重要**）

```
merge → propose import → review decide --accept-all → proposal apply → version freeze → 导出 TTL
```

- **不签** doc 条目 ⇒ TTL 里**没有**它们
- **签一条** doc 条目 ⇒ TTL 里**出现**它
- 判据：两次导出，grep 具体名字；证据是真实 TTL

### AC-008 · 真实 fixtures 跑通

- 用 `code/out/req116/cache/assertions.jsonl`（**56 行**）+ 现有 `proposal.json`
- 判据：给出统计五项（并入 / 排除 / 未映射 / 冲突 / both）

### AC-009 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` + `doc ingest` 的既有语义 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、新增依赖 |
| 出口 | 新命令登记进 `OUTLETS`（内部轨） |
| 基线 | 同机 `develop` 实测为准 |

## 二 · 纪律

- **H** 合并规则只许一处实现
- **G** 变异三元组：doc 条目设 strong / 猜表名 / 去掉 `applicable` ⇒ 每条必须被抓
- **S** AC 也会错，发现直接说


---

## 三 · Dev 首轮交付与裁定（Q-1 / Q-2）

### 首轮结果

| 项 | 值 |
| --- | --- |
| 并入 | 14（14 条「客户」断言并进 `customers`，`origin: both`） |
| 排除不可对齐 | 28（future 14 / example 14） |
| 未映射 | 14（「客户-下单->订单」没有 `maps_to`） |
| 冲突 | 0 |
| 确定性 | merged.json 两次 sha256 相同 |
| AC-006 | applier **拒绝** `applicable:false` 并点名（exit 1，不是忽略） |
| AC-007 | 不签：关系 52、grep 0 命中；签一条：**53**、TTL 里出现该边 |

### Q-1 · **签字没有一等入口**（必修）

Dev 自报：AC-007 的「签」是**手工改提案文件的 `reviewed_by`**，`review decide` 写的是 DB 双轨，两者不是一回事。

**裁定：加文件级签字命令**

```bash
ontok doc sign --file <merged.json> --item <序号或稳定键> --by <签字人> --out <signed.json>
```

| # | 约束 |
| --- | --- |
| 1 | `--by` 必填，匿名拒绝 |
| 2 | `--item` 支持序号与**稳定键**（如 `relation:订单->套餐模板`），⛔ 不许只按数组下标 |
| 3 | 只允许签 `doc` / `both` 条目 |
| 4 | 签完其余字节不变；重复签幂等 |
| 5 | ⛔ 不许就地改输入（必须给 `--out` 或显式 `--in-place`） |
| 6 | 确定性逐字节相同 |

**为什么不改成「apply 读 DB 双轨」**：`reviewed_by` 是 ADR-005 的**溯源字段**（谁认可了这条 AI 提案），换成读 DB 状态会丢掉「谁签的」。

### Q-2 · **父端列校验靠约定**（必修）

提案里没有主键列（抽取侧剥掉了 PK），所以 `customers.id` 这类父端列无法校验；Dev 退化成「列在 properties 里」或「列名恰好是 id」+ stderr 警告。

**裁定：加可选 `--schema`**

| 情形 | 行为 |
| --- | --- |
| 给了 `--schema` | **严格校验**（含父端主键列），不存在 ⇒ `doc_unmapped`，**0 例外** |
| 没给 | 保留 fallback，但 `stats` 必须有显式字段 + stderr 警告 |

⇒ **AC-004 订正**：不是「任何情况下都 0 例外」，而是「**给了 `--schema` 时 0 例外**；没给时 fallback 必须可数可见」。
