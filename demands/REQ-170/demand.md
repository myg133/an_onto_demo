# 需求：REQ-170 · 让构建链带上「库结构」（抽取能对齐表列 → R2RML 能生成）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-170 |
| 优先级 | **P1**（决定"纯本地一条命令"能不能产出**可查库**的本体） |
| 状态 | 已就绪 |
| 基线 | `develop` `7323382`（v0.3.0） |
| 触发 | 用户 2026-10-27 21:25：「现在的抽取里面，有数据库的关联吗？这个功能之前是有的」+「schema 的关联没有了，R2RML 还能做吗？」 |

## 问题（有源码证据）

**机制在，链路断在两个地方。**

### 断点 ①：抽取看不到库 ⇒ 产不出 `maps_to`

| 事实 | 证据 |
| --- | --- |
| `doc_merge` 要求关系断言自带 `maps_to`（child 列 + parent 列），没有 ⇒ `doc_unmapped`，⛔ 不猜表 | `src/extract/doc_merge.rs:20`（`maps_to` 在该文件出现 **70** 处） |
| 但**抽取侧从不产出 `maps_to`** | `src/extract/**` 里 `maps_to` 出现 **0** 处 |
| 契约（给 agent 的 skill）也没提 `maps_to` | `Deploy/skills/ontok-extract/SKILL.md` **0** 处 |
| `doc ingest` **没有任何 schema 参数** | `doc ingest --help`：`--input --out --stats --budget-tokens --limit-chunks --cache --chunks-only --agent* --strict-deterministic` |

⇒ 消费方在等一个生产方从不给的东西。文档里写明了后果（`doc_merge.rs:40`）：
> 概念永远命中不了、关系永远没有 `maps_to` ⇒ **并入 0、全量未映射**

这正是 REQ-147 里「**与真实表对齐 3 / 755**」的主因（当时误归因为"内置 LLM 质量差"）。

### 断点 ②：`build run` 不带库 ⇒ 根本没有 `schema.json` ⇒ R2RML 无从谈起

| 事实 | 证据 |
| --- | --- |
| `build run` 四段 = plan(读目录) → doc ingest → doc merge → 写草稿 → 起评审台，**全程不调 introspect** | `src/review_open/build_run.rs`（REQ-163 之后） |
| `r2rml generate` 的 `--schema` **必填** | `r2rml generate --help`：「没有它就没法跟库对账（⛔ 不猜主键）」 |

⇒ 纯文档项目（`build run` 产出的）里，**db 类查不了**：`R2RML` 生成不了 → `SPARQL` 连带不可用。
（AM / api 类取数不受影响：它只认本体 `class` + `iri_template`。）

## 要做四件

### R1 · `build run` 能带库

```
build run --input docs/ --work-dir demo/ --agent acp … [--dsn <库 URL> | --schema <schema.json>]
```

| 要求 | 细节 |
| --- | --- |
| `--dsn` | plan 阶段调**已有的** `extract::introspect::introspect(url)`（只读、fail-closed），产出 `<work-dir>/schema.json` |
| `--schema` | 直接用现成文件（人已经跑过 `introspect`） |
| 二者互斥 | 同时给 ⇒ exit 2 点名 |
| 都不给 | **行为与 REQ-163 逐字节不变**（纯文档路径零回归） |
| ⛔ 不碰 `src/db/**` | `introspect` 在 `src/extract/introspect.rs`（构建面），调它即可 |

### R2 · `doc ingest` 把库结构随块下发

```
doc ingest --input docs/ --out a.jsonl --schema <schema.json> --agent acp …
```

| 要求 | 细节 |
| --- | --- |
| 下发内容 | schema 的**摘要**（表名 + 中文名 + 列名 + 列类型 + 既有关系），随每块进 prompt（与 skill 同一条通道） |
| 指纹缓存 | schema 内容进 `prompt_sha`（换 schema ⇒ 缓存失效，和换 skill 同一条纪律） |
| 大小闸 | schema 摘要要压缩到可控体积（大库只带**相关的**表：按概念名/别名模糊匹配挑 top-N，N 可配）；⛔ 不许把 300 张表全塞进每块 |
| 不给 `--schema` | **产物逐字节不变** |

### R3 · 契约补「如果给了你表和列」（**BA 自己改**，不进本单的代码改动）

`Deploy/skills/ontok-extract/SKILL.md` 增一节：

- 概念名**优先对齐**表名 / 中文名（语义相同就沿用表的说法）
- 关系**必须**给 `maps_to`：`{ "child_column": "<列>", "parent_column": "<列>" }`，两端都要在 schema 里查得到
- 列不存在 ⇒ **丢弃该关系**并计数（⛔ 不许编列名、不许按名字猜表）
- 没给表列时：⛔ 不许输出 `maps_to`

### R4 · 端到端要能出可查的 R2RML

| 判据 | 内容 |
| --- | --- |
| 1 | 同一份文档 + 同一份 schema ⇒ 关系断言里**出现 `maps_to`**，且**每一条**都能在 schema 里查到（表存在、列存在） |
| 2 | `r2rml generate --project <p> --version <v> --schema <schema.json>` 能对这份定版**跑通并出 TTL** |
| 3 | 出的 TTL 能被**服务面只读解析**（拿 `ontok` 的解析路径读一遍即可；⛔ 不改服务端代码） |
| 4 | 无 `--schema` / 无 `--dsn` ⇒ 与 REQ-163 的产物**逐字节相同** |

## 范围禁令

⛔ 不动：`src/query/**`、`src/server/**`、`src/mcp/**`、`src/studio/**`、`src/db/**`、`src/provision/**`、`src/console/**`。
✅ 可改：`src/extract/**`（`doc_ingest` / `doc_acp` / `doc_merge` 的调用点）、`src/cli.rs`、`src/review_open/build_run.rs`、`src/domain/r2rml.rs`（如需）、测试。
⛔ 不改 `Deploy/skills/ontok-extract/SKILL.md`（BA 改，避免与 skills 分支冲突）。

## 背景：为什么这是"补链路"而不是"加功能"

这三样**都已存在**：`introspect`（库→schema）、`doc_merge`（吃 `maps_to`）、`r2rml generate`（schema+本体→TTL）。
缺的只是**把它们连起来**：抽取知道库、`build run` 带上库。
