# 验收标准：REQ-144

## 一 · AC

### AC-001 · 契约形状可解析

- 一份 `access.json`（含 1 个 source / 1 个 entity / 0-1 个 relation）能被加载
- 缺必填字段 ⇒ 报错**点名是哪个字段、在哪个 entity**
- 判据：贴一份成功 + 一份失败原文

### AC-002 · `validate` 四条检查各有牙

| # | 检查 | 构造 | 期望 |
| --- | --- | --- | --- |
| 1 | `iri_template` 与本体不一致 | 改 template | 报错（R3） |
| 2 | `pushdown` 与 `local_only` 重叠 | 同一字段写两处 | 报错（R4） |
| 3 | relation 的 `from`/`to` 不在 entities | 写个不存在的类 | 报错（R7） |
| 4 | `fields.x.path` 在 fixture 里找不到 | 写个不存在的路径 | 报错（**这条最有价值**：能发现映射写歪） |

- 判据：四条各一次，报错可行动

### AC-003 · `fetch --dry-run-fixture` 能跑通

- 用 fixture ⇒ 返回 JSON-LD（`@context`/`@graph`/`meta`）
- **⛔ 不发任何网络请求**（可用"断网/无效 base_url 也能跑"证明）
- 判据：输出 + 无效 `base_url` 仍成功

### AC-004 · 分页与本地过滤（核心语义）

- fixture 里有 250 条、`max_limit=100` ⇒ 应发 3 页（模拟），最终按 `--limit` 截断
- 过滤条件在 `local_only` 里 ⇒ `meta.filtered_locally` 含该字段，且结果**已按它筛过**
- 判据：贴 `meta` 与筛后行数

### AC-005 · `max_scan_rows` 超限拒绝（R5）

- 需要扫描 > `max_scan_rows` 才能满足 ⇒ **报错** `SCAN_LIMIT_EXCEEDED`，⛔ 不给部分结果
- 判据：构造一次 + 报错原文

### AC-006 · 未签 entity 拒绝取数（R6）

- `reviewed_by: null` 的 entity ⇒ `fetch` 报 `UNREVIEWED_MAPPING`
- 签上后才能取
- **变异**：把 R6 去掉 ⇒ 测试必须红

### AC-007 · `iri_template` 与本体一致（R3 的端到端）

- 用 ops360 的 TTL（`ontok:instanceIri`）+ 对应 access.json ⇒ 生成的 `@id` **与 TTL 模板同形**
- 判据：贴两边字符串对比

### AC-008 · `meta` 三项齐全（R2）

- `filtered_locally` / `freshness` / `capability_warnings` 都在
- 判据：一次输出的 `meta` 原文

### AC-009 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新依赖（阶段 0 不发网络）；删测试；放宽断言 |
| 出口 | 新子命令登记 `OUTLETS` |
| 基线 | 同机 `develop` 实测（Dev 自报） |

## 二 · 纪律

- **G** 变异：①`validate` 不查 fixture 路径（AC-002#4 必红）②去掉 `max_scan_rows`（AC-005 必红）③去掉 R6（AC-006 必红）
- **S** AC 也会错，发现直接说
