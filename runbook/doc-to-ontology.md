# 文档 → 本体 操作手册（REQ-129 之后）

> 这条链路在 REQ-129 之前是**断的**：`doc ingest` 产出 `assertions.jsonl` 后没有任何命令能把它并进本体。
> 现在通了，但**语义对齐仍是人的事**。

---

## 一 · 六步

```bash
# ① 文档 → 断言（带逐字证据）
ontok doc ingest --input docs/ --out out/assertions.jsonl --stats out/doc-stats.json

# ② 断言 + 库内提案 → 合并提案
ontok doc merge --assertions out/assertions.jsonl \
                --proposal   out/proposal.json \
                --schema     out/schema.json \
                --out        out/merged.json \
                --stats      out/merge-stats.json

# ③ 逐条签字（**只签 doc / both 条目**）
ontok doc sign --file out/merged.json \
               --item "relation:API 调用明细→客户" \
               --by "张三@2026-10-24" \
               --out out/merged-signed.json

# ④ 入库提案
ontok propose import --file out/merged-signed.json

# ⑤ 评审（双轨：业务 / 数据）
ontok review decide --proposal p1 --project <项目>          # 或逐条
ontok proposal apply --proposal p1 --project <项目>

# ⑥ 冻结 + 导出
ontok version freeze --project <项目> --version <N>
ontok ontology export --project <项目> --version <N> --model-view --schema out/schema.json --out out/model.ttl
```

---

## 二 · 顺序很重要（会踩的坑）

### ⚠️ 先合并完，再签字

`doc merge` 会**清掉已签条目的签名**（`reviewed_by_reset` 计数会告诉你清了几条）。

这是**正确行为**——内容变了，原来的签名就不该继续代表"我认可过这版"。但流程上要注意：

```
❌ 先签 → 又跑一次 merge → 签名没了
✅ 文档定稿 → merge → 签字 → import
```

### ⚠️ 输出不许撞输入

`--stats` / `--out` 指向输入文件会**直接报错**（REQ-129 的 N-1 修的就是这个：曾经会静默覆盖提案）。

---

## 三 · 合并器做了什么、没做什么

### 做了（机械对齐）

| 规则 | 行为 |
| --- | --- |
| 只并 `alignable: true` | `future` / `example` 的断言被排除并计数 |
| doc 条目 **最高 medium** | ⛔ 不许 `strong`；`reviewed_by: null` |
| 关系必须有显式 `maps_to` | 没有 ⇒ `doc_unmapped`，**不发明表名** |
| 同实体合并 | 映射到同一张表 ⇒ `origin: "both"`，两侧证据都保留 |
| 冲突双保留 | doc 与 DB 的基数矛盾 ⇒ `conflicts[]` + `applicable: false` |
| 未签不进快照 | apply 时忽略 doc-only 未签条目 |

### 没做（**故意的**）

> ⛔ **不做语义猜测**：「文档说『套餐』」→ 库里 `package_templates`，这条判断**合并器不猜**。

理由：它不是字符串相似度问题，是业务判断。猜错的后果是"能跑、不报错、结果错"——这正是这套系统一直在防的那类失败。

**谁来做**：人（或将来 AI 提案 + 人签）。

---

## 四 · 两个命令的输入输出

### `doc merge`

| 参数 | 说明 |
| --- | --- |
| `--assertions` | `doc ingest` 的 JSONL（每行一条断言，带 `evidence.quote`） |
| `--proposal` | `ontok-propose` 的提案 |
| `--schema` | **可选但强烈建议**：给了就严格校验 `maps_to` 的表/列（含父端主键列）；不给只能靠约定 + 警告 |
| `--out` | 与 `proposal.json` **同形状**，可直接 `propose import` |
| `--stats` | 五项统计 + 明细 |

### `doc sign`

| 参数 | 说明 |
| --- | --- |
| `--item` | 序号（1-based）**或稳定键**（`relation:订单→套餐` / `entity:客户`）；⛔ 不只认下标 |
| `--by` | **必填**，空串拒绝（溯源字段，不许匿名） |
| `--out` / `--in-place` | ⛔ 不提供"默认就地改" |
| `--at` | 可选；省略取当前 UTC（`signed_at` 是**事实**，不是计算产物） |

---

## 五 · 真实 fixtures 的合并结果（56 行断言）

| 项 | 数 |
| --- | --- |
| 并入 | 14（14 条「客户」断言并进 `customers`，`origin: both`） |
| 排除不可对齐 | 28（future 14 / example 14） |
| 未映射 | 14（「客户-下单->订单」没有 `maps_to`） |
| 冲突 | 0 |
| both | 1 |

> **14/14 未映射不是坏事**——它说明合并没有瞎猜。要让它变成关系，得先有人把「下单」映射到具体的外键列。

---

## 六 · 已知限制

| # | 限制 |
| --- | --- |
| 1 | 只吃 `.md` / `.txt`；PDF/Word 需外部转换 |
| 2 | `alignable` 由 `doc ingest` 判（`future`/`example` 不可对齐），合并器信任这个字段 |
| 3 | 冲突条目**不能**被 `--accept-all` 顺手应用（apply 会拒绝并点名）——这是设计 |
| 4 | `signed_at_source`（时间是人给的还是机器取的）尚未记录 |
