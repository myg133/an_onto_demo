# 一、src/extract/propose.rs 391 行改动：逐块分类

范围：`git diff 90d9046..504ae9f -- src/extract/propose.rs` → 8 个 hunk，+288 / -103。
文件从 1108 行 → 1293 行。

## 逐块分类表

| # | hunk | 内容 | 分类 |
|---|------|------|------|
| 1 | `@@ -27,7 +27,7` | `use` 增加 `TableMeta` | 签名调整（新函数需要 `TableMeta` 入参） |
| 2 | `@@ -64,10 +64,30` | `ExtractedBy` → `ProposalProvenance`，+22 行改名的交叉引用文档 | 改名 + 文档；**字段零变化**（`agent`/`version`/`round`/`failed_groups`/`source` 全同） |
| 3 | `@@ -120,7 +140,7` | `Proposal.extracted_by` 字段类型跟随改名 | 签名调整（跟随 #2） |
| 4 | `@@ -128,6 +148,33` | 新增 `DEFAULT_PROPOSAL_ROUND` / `DEFAULT_PROPOSAL_BASE_VERSION` / `introspect_provenance()` | 新增；`introspect_provenance` 的 `format!` 与旧内联**逐字相同** |
| 5 | `@@ -286,15 +333,12` | `extracted_by` 构造：`round: 1` → `DEFAULT_PROPOSAL_ROUND`，内联 `format!` → `introspect_provenance(report)` | 纯搬移（值不变） |
| 6 | `@@ -318,17 +362,15` | 展示名内联 → `entity_display_name(t, &names)` | 纯搬移（见下） |
| 7 | `@@ -354,11 +396,214` | 属性循环 → `build_properties()`；新增 `PropertyOrigin`/`PropertyDraft`/三个新函数 + 文档 | 纯搬移 + 新增类型（**唯一一处 `stats.*` 写入被移出**，见下） |
| 8 | `@@ -456,92 +701,32` | `from_relation` 分类 + 关系循环 → `links_for_table()` | 纯搬移（`stats` 写入移出） |

**行为变更判定：全文件无行为变更。** 8 个 hunk 全部是「搬移 / 改名 / 加文档 / 加常量」。

## 关键语义等价性论证（逐条）

1. **`props_via_*` 三个计数**（hunk 7/8 交界）：
   旧：`if from_relation {relation+=1; wordlist+=1} else if name.is_empty() {wordlist+=1} else {comment+=1}`
   新：函数内算出 `origin`（三分支与旧判据**一一对应**），调用方按 `origin` 还原同样三个计数
   （`src/extract/propose.rs:404-412`）。因为 `PropertyDraft.item.name` 就是那个 `name`，
   两侧判据恒等。**等价。**
2. **`props_comment_rejected`**：旧在 match 臂内直接 `stats.* += 1`；新改为置 `comment_rejected = true`，
   调用方 `if d.comment_rejected { stats.props_comment_rejected += 1 }`（`propose.rs:416-418`）。
   该 match 臂**每个被访问的列只触发一次**，置位与自增次数一致。**等价。**
3. **`links_inferred`**：旧在 push 前 `+= 1`；新 `links.len()`（`propose.rs:429`）。
   被 `continue` 掉的自引用不计入，两侧一致。**等价。**
4. **`total_props`**：`props` 变量在两个版本里都是**每张表新建**（BASE:337 / HEAD:379），
   都在表循环内。**等价。**
5. **AC-007「两端在实体集合」自检**：旧在 `extract_with_context` 末尾，新**仍在其末尾**
   （`propose.rs:433-441`），只是同时被 `assemble::rebuild_outcome` 复用。**等价。**
6. **`base_version: 1` / `round: 1`**：hunk 显示这两行仍是**字面量 `1`**，未被新常量替换
   （见「发现 D1」）。
