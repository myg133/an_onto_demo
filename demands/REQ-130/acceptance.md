# 验收标准：REQ-130

## 一 · AC

### AC-001 · N-3 判据改成按条目匹配

- 新判据断言：`merged.json` 里**不存在** `name == <被排除概念>` 的 concept 条目、**不存在** `subject/object` 含它的 relation 条目
- ⛔ 不再用整文件 grep
- 判据：真实 fixture 上跑，**绿**；同时 `excluded_not_alignable == 28` 仍成立

### AC-002 · N-3 的反向判据（防"改松了"）

- 手工往 `merged.json` 塞一条 `alignable:false` 的**条目** ⇒ 新判据**必须红**
- 判据：这是"判据有牙"的唯一证明

### AC-003 · N-2 字段

- 给 `--at` ⇒ 产物含 `signed_at_source: "arg"`
- 省略 `--at` ⇒ 产物含 `signed_at_source: "system"`
- 判据：两次各一，字段值正确

### AC-004 · 确定性与兼容

- 同参数（含 `--at`）两次 sign ⇒ 逐字节相同
- 读回**缺少** `signed_at_source` 的老产物 ⇒ 不报错（按缺省处理）
- 判据：各一次

### AC-005 · 不退化

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` + `doc merge/sign` 的其他行为 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言（**这次是收紧判据，不是放宽**）、新增依赖 |
| 基线 | 同机 `develop` 实测 |

## 二 · 纪律

- **G** 变异：把新判据改回 grep ⇒ AC-002 必须红
- **S** AC 也会错，发现直接说
