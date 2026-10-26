# 状态：REQ-157 · 关系动词治理

## 当前状态

**进行中**（2026-10-27 · P1+P2 已派 Dev；P3 由 BA 跑）

## 三期

| 期 | 内容 | 状态 |
| --- | --- | --- |
| P1 | 字典 + `relation verbs check` + `semantic_cn` 改从字典读 + **altLabel 纳入守卫** | 🟡 跑着 |
| P2 | `parse.rs` schema 去掉枚举 + skill 补动词字典 | 🟡 跑着 |
| P3 | 重抽 v4 → **产出提案，不代签** | ⬜ BA 来跑 |

## 关键约束

- ⛔ **不代签 `reviewed_by`**，⛔ 不冻结 v4（那是人签的事）
- ⛔ 不改已冻结的 v3
- 泛词（`uses`/`associated_with`/`related_to`）**一律拒绝**，不是警告
- 字典外的词拒绝 + 给 3 个候选（便于人改）

## 起点数据（v3 现状）

15 种动词：`belongs_to 16 · references 4 · binds_to 2 · attached_to 1 · attaches_to 1 · notifies_via 1 · applies_to 1 · targets 1 · binds 1 · uses 1 · contains 1 · associated_with 1 · bills 1 · based_on 1 · deployed_to 1`

其中需清理：`attaches_to`（同义）· `binds`（同义）· `uses`/`contains`/`associated_with`（泛词）


## P3 的执行计划（BA 自己跑，不需要 Dev）

1. `relation verbs check` 对 v3 跑一遍 → 拿到 offenders 清单（预期：`forbidden` 3、`alias_normalized` 2）
2. 针对每个 offender，用**语料 + 新 skill + 字典**跑一次**定点重命名**（只处理那几条，不重抽全部）
3. 产出提案文件 → **再过一遍 `check`**（必须 exit 0）
4. 落盘 `BA/demands/REQ-157/proposal-v4-naming.json` + 一份人话报告
5. **停在待签**：⛔ 不写库、⛔ 不冻结 v4、⛔ 不代签 `reviewed_by`
