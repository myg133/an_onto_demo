# REQ-179 · 本体 × RAG：**知识问答**（草稿，等开工信号）

- 状态：**草稿**（2026-10-29 与用户对齐了方向，尚未派单）
- 触发：用户问「我们目前的本体能不能跟 RAG 关联上」，逐项确认后记录

## 一 · 已确认的四个口径（用户拍板）

| # | 问题 | 用户答复 |
| --- | --- | --- |
| 1 | RAG 用来干什么 | **知识问答**（要引用、要防幻觉） |
| 2 | 向量库谁提供 | **我们自己做到系统内部** |
| 3 | 关系 evidence 怎么补 | **采样 + 人签** |
| 4a | 原文从哪来 | **从抽取本体所用的那批文本文档** |
| 4b | embedding 谁算 | **外部模型 API** |

## 二 · 关键认知（写死，防止做歪）

> **本体的 `description` 不是证据。** 它是抽取时 LLM 写的摘要，只能当**索引/召回线索**；
> 回答里的每一条断言都必须回到**原文 chunk**，并带 `[文件:行]`。

我们手上已有的、别人做 GraphRAG 最费劲的东西：

| 资产 | 用途 |
| --- | --- |
| 描述里的 `（cvm.md:337）` 出处标注 | **chunk ↔ 概念** 的精确锚点（正则可抽） |
| `aliases` / `display_name` | 实体链接词典（query 词 → 概念） |
| `evidence` 等级 | 决定"能不能断言" |
| `@pending.get` 配方 | "多少/有没有"这类问题取真数 |
| `catalog` / `describe` / `/mcp` | 结构化检索通道（已在线） |

## 三 · 设计要点（草案）

### 3.1 三张新表（挂项目/版本）

```
documents(project_id, path, sha256, text)
chunks(document_id, seq, start_line, end_line, text, embedding bytea, dim)
concept_mentions(chunk_id, class_key|property_key, source)  -- provenance | alias | manual
```

`source=provenance` 由**正则解析**本体描述里的 `（文件:行）` 得到（零成本、精确）；
`alias` 用显示名/别名做近似锚定补齐。

### 3.2 向量存哪：**不用 pgvector**

实测：UAT 的 PG 16 **没有 `vector` 扩展**（`pg_available_extensions` 只有 `pg_trgm`）。
按我们的规模（2 万 chunk × 1024 维）**Rust 暴力检索只要几毫秒** ⇒
embedding 存 `bytea`，进程内建索引、按 `version_id` 懒加载并缓存。
将来规模上去再加 pgvector 作为**可选后端**，不破坏接口。

### 3.3 四路混合检索

```
问题
 ├─ ① 实体链接：aliases/display_name + pg_trgm → 命中概念
 ├─ ② 概念扩展：命中概念 → 1~2 跳关系（**只走 evidence 达标/已人签的边**）
 ├─ ③ 向量召回：chunk 检索，命中概念的 chunk 加权
 └─ ④ 取数分支：问数字 → @pending.get（已有）
        ↓
   回答：断言带 [文件:行]；数字标"实时"；检索不到就说不知道
```

### 3.4 防幻觉三条硬规则

1. **没检索到原文就不答**（"不知道"是合法答案）
2. **每条断言带 `[文件:行]`**，且该行区间必须真在 `chunks` 里
3. `evidence != true` 的关系只能写成"可能是"，⛔ 不能当断言

### 3.5 采样 + 人签（evidence 升级）

```
每个本体的关系（34~40 条）→ 分层抽样 20% → 自动实测（调 API / 查库）
   → 通过：evidence 升级 + reviewed_by 签名（复用现有 review 台）
   → 不通过：标 false，问答里不能说
```

## 四 · 分期（草案）

| 期 | 内容 | 估 |
| --- | --- | --- |
| **P0** | 三张表 + 包内带**原始文档** + provenance 正则解析 + 外部 embedding API 接线 + bytea 向量 + 暴力检索 | 3-4 天 |
| **P1** | 问答链路（实体链接 / 概念扩展 / 引用校验）+ `POST /api/ask` | 3-4 天 |
| **P2** | 采样实测 + 人签升级 evidence | 2-3 天 |
| P3（可选） | pgvector 后端 / 多跳优化 / 重排 | 视规模 |

## 五 · 未决（开工前要定）

1. **包格式**：原始文档要不要进 `.ontokbundle`？（现在只有快照）—— 涉及包体积与契约版本
2. **embedding 服务**：外部 API 的地址/模型/维度/限流/成本，谁提供凭据
3. **问答的鉴权**：走 query key（数字员工）还是单独一类 key
4. **原文版权**：客户文档进我们系统，存储与保留策略要明确
