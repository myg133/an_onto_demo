# 验收标准：REQ-143

## 一 · AC

### AC-001 · 开关默认关闭，行为零变化（R5）

- `doc merge`（不传 `--allow-doc-only`）用"空提案 + 3 条断言" ⇒ **并入 0 / doc-only 0 / 未映射 3**，与基线**逐字节相同**
- 判据：两次输出 sha256 + 三项计数

### AC-002 · 开开关后，概念能新建（纯文档通路成立）

- 同上输入 + `--allow-doc-only` ⇒ **新建实体 = 概念数**（可对齐的那些），`origin: "doc"`，`doc_unmapped` 只剩"关系无端点"的情况
- 判据：贴 `stats` 五项 + 一条新建条目原文

### AC-003 · key 确定性（R2）

- 同一输入跑两次 ⇒ `merged.json` **逐字节相同**
- 且 key **不含随机/时间/序号依赖输入顺序**的成分
- 判据：两次 sha256；并说明 key 的派生规则（`path:line`）

### AC-004 · band 与签名（R3）

- 所有新建（`origin: "doc"`）条目：`band ∈ {low, medium}`、`reviewed_by: null`
- **变异**：把 doc 条目设成 `strong` ⇒ 必须有测试红
- 判据：逐条断言 + 变异红行

### AC-005 · 关系不许造端点（R4）

- 断言里 `subject`/`object` 有一个解析不到 ⇒ **该关系进 `doc_unmapped`**，`@graph`/提案里**不出现**这条边
- 判据：构造一条"一端不存在"的关系，验证

### AC-006 · 端到端（**最重要**）

```
doc ingest → doc merge --allow-doc-only → propose import → review decide --accept-all
           → proposal apply → version freeze → ontology export
```

- 产出 TTL 里**类数 = 概念数**、关系数 = 可解析的关系数
- 与"用脚本转换"的老路径相比：**TTL 内容等价**（类/关系/属性计数一致）
- 判据：贴 TTL 头部计数 + `catalog` 的 counts

### AC-007 · 冲突与排除规则不变

- `alignable: false` 仍排除并计数
- 冲突仍双保留 + `applicable: false`（apply 拒绝）
- 判据：各一条

### AC-008 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新依赖；删测试；放宽断言；改 `doc ingest` |
| 基线 | 同机 `develop` 实测（**Dev 自报，BA 不预设**） |

## 二 · 纪律

- **G** 变异：①默认开（AC-001 必红）②key 带随机（AC-003 必红）③doc 条目设 strong（AC-004 必红）
- **S** AC 也会错，发现直接说
