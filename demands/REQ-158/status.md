# 状态：REQ-158 · 修 fail-open（TTL 形状的英文语义能过闸）

## 当前状态

**已完成**（2026-10-27 · 合并 `3b5b414` · 部署 **v0.2.7**）

## 结果

| 项 | 值 |
| --- | --- |
| Dev commit | `3e60f3b`（2 文件 / **+529 / −0**；`view.rs` 部分 **+102 / −0**） |
| 合并 | `3b5b414` · tag **v0.2.7** · digest `sha256:be50fc94…` |
| 新守卫 | `test_req158_semantic_comment_gate` **9/9 绿**（需 `ONTOK_TEST_DATABASE_URL`，不带则那条端到端红——BA 已复现并确认） |
| 既有守卫 | `test_req156_semantic_cn_invariants` 5/5 · `test_req107_red_lines` 2/2 · 全绿 |
| 全量 | **1937 passed / 8 failed / 3 ignored**（8 条与基线逐条同名同因） |
| 线上 | health/catalog/describe/TTL **零变化**；34 条关系仍全中文标签 |

## 实现偏离（已接受）

原计划把 TTL 形状 chain 进 `untranslated_semantic_hit`，但**两道守卫同时钉着它**：
- `test_req107_red_lines.rs:88` 要求 `view.rs` 的 diff **删除行数 == 0**（REQ-156 的"只许追加"授权）
- `test_req156_semantic_cn_invariants.rs:151` 用 `body_of()` **逐字锚定**该函数签名与函数体

⇒ 任何 chain 写法都要替换既有行，同时打红两道守卫。**改成"行为等价、位置不同"**：在 `scan_modeling_terms` 里追加**第四档-b**，判据与文案同形；`meaning_values` / `untranslated_semantic_hit` / `semantic_cn` **一个字节未动**。

**裁定：接受。** 理由：偏离的是**位置**，不是**判据**；且它把"不许改既有防线"这条纪律守住了。

## 真进程实证（最关键的一条）

| | 旧代码 | 修后 |
| --- | --- | --- |
| 含 `语义：aggregates` 的客户视图 | **exit=0，真的写出**了 `rdfs:comment` + `skos:altLabel` | **exit=2**，stderr 点名 `aggregates` + 出口 C6，**哨兵文件逐字节未动** |

## 遗留两条（已进 REQ-157 的验收）

| # | 项 | 处置 |
| --- | --- | --- |
| 1 | `skos:altLabel "…"@zh` 是**同一值的第二种形状，零覆盖**（`ttl.rs:2068`）。今天被第四档-b 遮住；一旦有人改掉 `rdfs:comment` 那句，它立刻变成新的 fail-open | **进 REQ-157 AC** |
| 2 | `/api/catalog` 的 `semantic` 是**英文机器键**（REQ-156 的产品决定），且该路径不走 `scan_modeling_terms` | **判定为设计如此**（`semantic_label` 才是给人看的） |
