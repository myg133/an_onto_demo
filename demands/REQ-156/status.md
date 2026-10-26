# 状态：REQ-156 · 透出关系语义动词

## 当前状态

**进行中**（2026-10-27 · 已派 Dev）

## 起点事实

- 本体里**已有** `semantic`：34 条 / 15 种动词
- `/api/catalog` 的 relation 项**没有**这个字段 ⇒ Studio 只能显示 `from__to`
- 中文映射 `semantic_cn()` 只覆盖 **6/15**，其余 9 个英文漏出
- ⚠️ 已有的守卫 `untranslated_semantic_hit`（英文即拒）**在冻结时跑**，v3 是守卫之前冻的，之后没重跑

## 分工

| 单 | 内容 |
| --- | --- |
| **REQ-156**（本单） | 透出 `semantic` + `semantic_label`；补齐 15 个词的映射；Studio 显示中文 |
| REQ-157（后续） | 动词字典 + 校验拒绝未知词 + skill/schema 对齐 + 重抽 v4（泛词归零、同义不并存） |
