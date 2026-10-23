# 状态：REQ-128 · `--names` 必填

## 当前状态

**进行中**（2026-10-24 开单）

## 为什么要做

`ontok-propose` 的 `--names` 默认值是 `src/extract/table_names.json` —— 那是**脱敏占位样例**（91 条 `t001 → 对象 001`）。

| 场景 | 后果 |
| --- | --- |
| 容器里不传 | 去 `/data/src/extract/...` 找 → **文件不存在** |
| 仓库根不传 | 读到占位表 → 实体名全是「对象 001」→ **白跑，且不报错** |

不是「被带偏」，是**静默白跑**。

## 顺带查清的（不喂给 agent）

| 文件 | 运行时去向 |
| --- | --- |
| `concept_groups.transition.json` | 只在 `concept.rs:910` 的 `#[cfg(test)]` 里 `include_str!` |
| `query-policy.example.json` | 只在缺 `--policy` 时当错误模板打印 |
| `ontok-gate-preflight.rs` | 测试当数据做行号锚点比对 |

## 待办

- [ ] Dev：必填 + 可行动错误 + 占位检测警告 + 修受影响测试
- [ ] QA
