# 验收标准：REQ-139

## 一 · AC

### AC-001 · 两个字段都在（本体模式）

- `--ontology-only` 起 ops360 ⇒ `GET /api/health` 含 `"project": "ops360"`、`"ontology_version": 2`
- 判据：响应原文

### AC-002 · 普通模式也有（R3）

- anspire 项目 ⇒ `project` / `ontology_version` 都正确
- 判据：响应原文

### AC-003 · 与 catalog 一致（R4）

- `/api/health` 的 `project`/`ontology_version` == `/api/catalog` 的 `project`/`version`
- 判据：两处数值对比（不是"看起来一样"）

### AC-004 · 现有字段一个不少（R5）

- `service` / `version` / `syntax` / `protocols` / `transports` 原样；`mode` 仍只在本体模式出现
- 判据：与基线响应做键集合对比

### AC-005 · 不泄数据（R2）

- health 里 grep：租户 uuid、连接串、表名（`documents`/`customers` 等）、`password` ⇒ **0 命中**
- 判据：grep 输出

### AC-006 · 免 token

- 不带 Authorization ⇒ 200
- 判据：一次

### AC-007 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新依赖；删测试；放宽断言 |
| 基线 | 同机 `develop`；`/metrics` 与 catalog/TTL/SPARQL 的字节不变 |

## 二 · 纪律

- **G** 变异：把 `ontology_version` 恒写 `0` ⇒ AC-003 必须红（说明那条断言不是空转）
- **S** AC 也会错，发现直接说
