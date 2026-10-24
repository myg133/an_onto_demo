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
