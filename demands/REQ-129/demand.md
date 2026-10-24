# 需求：REQ-129 · 文档断言合并进本体提案

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-129 |
| 优先级 | **P0**（文档这条路现在断在最后一步） |
| 状态 | 进行中 |
| 基线 | `develop` `8774034` |
| 前置 | REQ-116（`doc ingest` → assertions.jsonl）、REQ-118/120（提案形状） |

## 问题

```
文档 → ontok doc ingest → assertions.jsonl（带逐字证据）
                                    │
                                    ⚠️ 断在这里
                                    ▼
                          没有任何命令把它并进提案
```

`ontok doc --help` 自己写着「**只做第一步**」。

⇒ 文档抽出来的概念/关系**进不了本体**，只能人工看 JSONL。

## 做的范围（**严格只做第一步**）

新增一条命令：

```
ontok doc merge --assertions <assertions.jsonl> \
                --proposal   <proposal.json> \
                --out        <merged.json> \
                [--stats <stats.json>]
```

**产出与 `proposal.json` 同形状**，因此可以继续走既有的 `propose import → review decide → proposal apply → version freeze`。

### 合并规则（逐条钉死）

| # | 规则 |
| --- | --- |
| 1 | **只合并 `alignable: true` 的断言**；`future` / `example` 一律排除并计数（它们是文档里的设想/举例） |
| 2 | **doc 来源条目最高 `medium`**（`reviewed_by: null`）；⛔ 不许 `strong` |
| 3 | **关系必须有显式映射**（child 列 + parent 列）；没有映射 ⇒ 进 `doc_unmapped`，⛔ **不许发明表名** |
| 4 | **同实体合并**：doc 概念与 DB 实体映射到同一张表（或名字/别名命中）⇒ `origin: "both"`，两侧证据都保留 |
| 5 | **冲突双保留**：doc 的基数断言与 DB 推断不一致 ⇒ 生成 `conflicts[]` 条目，**两边都留**，且该条目**不可被 apply** |
| 6 | **确定性**：同一输入两次运行 ⇒ **逐字节相同**（排序稳定、无时间戳、无随机） |
| 7 | **未签不进快照**：doc 条目 `reviewed_by: null` ⇒ apply 时被忽略（复用 REQ-118 的 P2） |

### 关键裁定：**"文档里的词 = 库里哪张表"由人签**

- 合并器只做**机械对齐**：断言若已带 `maps_to`（表/列），校验存在性后并入；不存在 ⇒ `doc_unmapped`
- ⛔ 合并器**不做语义猜测**（不许「『套餐』这词像 `package_templates` 就自动映射」）
- 语义对齐由人（或后续的 AI 提案 + 人签）完成

## 不做

- ⛔ 不做语义映射猜测
- ⛔ 不改 `doc ingest`（它是第一步，已完成）
- ⛔ 不改提案/评审/应用的既有语义
- ⛔ 不引新依赖

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 合并后提案膨胀，人审负担上升 | 只并 `alignable`；doc 条目单独分组显示（`origin: doc`） |
| 2 | 冲突条目被误 apply | 冲突条目显式标 `applicable: false`，applier 拒绝 |
| 3 | 同一实体被 doc 与 DB 各建一次 | 规则 4 的 `both` 合并 + 去重键 |
| 4 | 确定性被排序细节破坏 | 全部集合按稳定键排序；AC 用逐字节对拍 |

## 交付物

1. `ontok doc merge` + 合并器
2. 真实 fixtures 跑通（`code/out/req116/cache/assertions.jsonl` 56 行）
3. 合并统计：并入 / 排除（不可对齐）/ 未映射 / 冲突 / 合并为 both
4. 端到端：merge → import → 人签 → apply ⇒ TTL 里出现 doc 条目；不签则不出现
5. 已知限制
