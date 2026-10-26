# 状态：REQ-158 · 修 fail-open（TTL 形状的英文语义能过闸）

## 当前状态

**已就绪**（2026-10-27 · hotfix 级 · 未开工）

## 起因

REQ-156 期间实测：v3 客户视图 TTL 里有 **13 个英文动词（14 行）**，而"漏翻即拒绝"的守卫
`untranslated_semantic_hit` **只认 JSON `"meaning"` 键**，对 TTL 的 `rdfs:comment "语义：…"` 形状**完全看不见**。

## 影响

客户拿到 `/api/ontology.ttl` 能看到 `applies_to` / `attached_to` / `uses` 这类**内部标识符**。

## 判据（能红能绿）

- TTL 探针串（含 `语义：applies_to`）⇒ **必须命中**
- 原 JSON 探针串（含 `"meaning":"applies_to"`）⇒ **仍必须命中**
- 中文语义（`语义：归属于`）⇒ **不许误报**
- 端到端：含英文语义的视图 ⇒ 发布被拒并点名
