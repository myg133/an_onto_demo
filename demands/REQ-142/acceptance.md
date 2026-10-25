# 验收标准：REQ-142

## 一 · AC

### AC-001 · REST 默认是 JSON-LD

- `POST /api/sparql` **不带任何 format 线索**（body 无 `format`、无 `?format`、无 `Accept`）⇒ 返回 JSON-LD
- 判据：贴顶层键（应为 `@context` / `@graph` / `summary` / `meta`）

### AC-002 · `Accept` 判定

| `Accept` | 期望 |
| --- | --- |
| `application/ld+json` | JSON-LD |
| `application/json` | **rows** |
| `*/*` | **JSON-LD**（默认） |
| 缺失 | **JSON-LD**（默认） |

- 判据：四行各一次

### AC-003 · `?format` 与 body `format` 优先于 `Accept`

- `Accept: application/json` + `?format=jsonld` ⇒ **JSON-LD**
- `Accept: application/ld+json` + `?format=rows` ⇒ **rows**
- 判据：两行各一次（证明优先级）

### AC-004 · MCP 默认是 JSON-LD

- `sparql_query` **省略 `format`** ⇒ `structuredContent` 是图
- `format: "rows"` ⇒ 是表
- 判据：两次

### AC-005 · 退回 rows 的输出**逐字节等于旧默认**（R2）

- `?format=rows` 与基线 `be483e7` 的默认输出 ⇒ sha256 相同（3 条查询）
- 判据：3 × 2 的 sha 对比

### AC-006 · JSON-LD 内容不变

- 默认切换前后，同一查询的 JSON-LD 输出**逐字节相同**（与基线 `be483e7` 的 `?format=jsonld` 对比）
- 判据：sha 对比

### AC-007 · 非法 format 仍 400（R3）

- `?format=bogus` ⇒ 400 + 可行动 hint（⛔ 不静默回退）
- 判据：状态码 + 原文

### AC-008 · 受影响测试的改法（R4）

- 逐条列出：哪些测试原来假设默认 rows ⇒ 现在**显式传 rows**（断言不变）
- 且**新增**至少 1 条"默认是 jsonld"的判据
- ⛔ 不许删测试、不许放宽断言
- 判据：diff 清单 + 测试总数不减少

### AC-009 · 内部消费者已排查

- 全仓 grep 调 `/api/sparql` 与 MCP `sparql_query` 的地方，逐个说明"是否需要显式 rows"
- 判据：清单（文件:行 + 结论）

### AC-010 · 文档与契约（R5）

- skill 两份写明"默认是图；要表传 `format=rows`"
- REQ-141 契约里的默认值与本需求一致（若 141 尚未合入，在交付里注明"契约文案待 141 落地"）
- 判据：贴文案改动

### AC-011 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新依赖；删测试；放宽断言 |
| 基线 | 同机 `develop` 实测（**以 Dev 自报为准，BA 不预设数字**） |

## 二 · 纪律

- **G** 变异：把 `*/*` 的判定改回 rows ⇒ AC-002 必须红
- **S** AC 也会错，发现直接说
