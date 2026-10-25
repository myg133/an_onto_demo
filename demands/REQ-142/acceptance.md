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


---

## 三 · 裁定（Dev 提的 5 处）

### R-1 · 优先级：**保留 `?format=` > body.format**（我 demand 里写反了）

我在 demand 里写「body.format 第一，`?format` 第二」，但 **REQ-140 已实现并已断言 `?format` > body**。

**裁定：按代码现状（`?format` > body），订正需求的文字。**

| 理由 | 说明 |
| --- | --- |
| 本需求的范围是**翻默认**，不是**改优先级** | 改优先级 = 改既有行为 + 翻 REQ-140 的一条断言，**超出授权** |
| 两种都站得住 | `?format` 是传输层提示，body 是请求体；没有明显对错 |
| **一致性 > 理想** | 已发布且有测试钉住的行为，不因"我觉得更合理"而改 |

⇒ **执行优先级（最终，以代码为准）**：

```
?format=   >   body.format   >   Accept   >   默认 jsonld
```

### R-2 · 两条"默认值本身"的断言被翻转：**接受**

它们断言的就是「默认是什么」。默认变了，断言必须变——**这不叫放宽，叫跟着事实走**。
关键是：其余 6 条是**加显式 rows、断言零改动**，这个边界 Dev 划得对。

### R-3 · `initialize.instructions` 字节变化：**接受**

它是**契约文案**，不是数据回执。R5 要求写明默认，文案就该变。

### R-4 · `Accept` 同时含 `ld+json` 与 `application/json` ⇒ 取 JSON-LD：**接受，但必须写进契约**

理由：`application/ld+json` 是**更具体的媒体类型**，具体压过一般。这条需求没写，属合理补充。
⇒ REQ-141 的契约 `notes` 里要补一句。

### R-5 · live ac003 因"只有一个有数据的租户"而 SKIP：**接受**

环境限制（真实库只有一个租户有数据），不是实现问题。已在响亮 SKIP 里说明即可。

### ⚠️ R-6 · Dev 指出的**外部文档未改**（这条是真欠账）

Dev 明确列出（都在 worktree 外，它不能改）：

| 文件 | 问题 |
| --- | --- |
| `e2e-test/query/BRIEF.md:20`、`e2e-test/agent-workspace/RULES.md:8` | 还写着"默认 rows"，下轮 e2e Agent 会**拿图当表读** |
| `BA/runbook/ontology-e2e.md:292,300,326` | 示例命令未写 format |
| `Deploy/skills/ontok-query/SKILL.md`、`ops360-ontology/SKILL.md`、`README.md` | 文案未更新 |

**处置：合入后由 BA 统一改**（这些是交付资产，不在 feature worktree 里）。⛔ 不能让它们继续写错——**这正是"文档与实现漂移"的典型发生点**。
