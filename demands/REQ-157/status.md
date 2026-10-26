# 状态：REQ-157 · 关系动词治理

## 当前状态

**已就绪**（2026-10-27 · 等 REQ-158 完成后开工）

## 三期

| 期 | 内容 | 状态 |
| --- | --- | --- |
| P1 | 字典 `relation-verbs.json` + `relation verbs check` 命令 + `semantic_cn` 改从字典读 | ⬜ |
| P2 | `parse.rs` schema 去掉枚举 + skill 补动词字典 | ⬜ |
| P3 | 重抽 v4 → **产出提案，不代签** | ⬜ |

## 关键约束

- ⛔ **不代签 `reviewed_by`**，⛔ 不冻结 v4（那是人签的事）
- ⛔ 不改已冻结的 v3
- 泛词（`uses`/`associated_with`/`related_to`）**一律拒绝**，不是警告
- 字典外的词拒绝 + 给 3 个候选（便于人改）

## 起点数据（v3 现状）

15 种动词：`belongs_to 16 · references 4 · binds_to 2 · attached_to 1 · attaches_to 1 · notifies_via 1 · applies_to 1 · targets 1 · binds 1 · uses 1 · contains 1 · associated_with 1 · bills 1 · based_on 1 · deployed_to 1`

其中需清理：`attaches_to`（同义）· `binds`（同义）· `uses`/`contains`/`associated_with`（泛词）
