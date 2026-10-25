# 需求：REQ-148 · 文档本体 ↔ 数据表对齐（打通"文档建本体 → 映射数据"）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-148 |
| 优先级 | **P0**（挡住"完整演示场景"的那一步） |
| 状态 | 已就绪（未排期） |
| 基线 | `develop` `6f4987d` |
| 触发 | 用腾讯云文档建出 **30 类**本体（v3）后，**无法与真实数据（14 表）合成一个本体** |

## 现状（实测，不是推测）

```
extracted v2 = 从真实表建的 14 类（已冻结）
extracted v3 = 从 795 个腾讯云类型里选出的 30 类（挂在空 v1 上）
```

**v2 与 v3 是兄弟，不是父子**——因为产品缺四个能力。

## 要做的五件（按优先级）

### G1 · `version branch --from <seq>`

```
ontok version branch --project <p> --from 2 --out 4
```

**现状**：`version new` 只建**空**版本；已冻结的 v2 不能被派生。

**实测报错**：
```
✗ 提案基于 v2，而 v2 已冻结，不能在其上提案。
```

⇒ 冻结=不可再生，这在真实使用里是致命的（"我在 v2 上加一个类"做不到）。

### G2 · 文档概念 ↔ 已有实体对齐（**核心**）

```
ontok doc align --proposal p1 --project <p> --version <v> --mapping align.json
```

`align.json` 声明"文档类 A 就是本体的实体 K"：

```json
{ "alignments": [
    { "doc_class": "Instance", "entity": "cvm_instance",
      "attributes": { "InstanceId": "instance_id", "InstanceState": "status" } } ] }
```

**现状**：9 个同名 key 会撞车（`key 冲突：cvm_instance 已存在于该版本`）。
**后果**：文档本体和数据库本体**永远合不到一起**，用户只能二选一。

### G3 · 关系证据能带文档出处进入快照

**现状**：快照 `Evidence` 只认 `{table, column, claim}`，且**要求非空列**。
⇒ 我们的 34 条文档关系，进快照后 `evidence = []`——**859 条行号证据全丢了**。

**要**：`Evidence` 支持 `{file, line, quote}` 变体（二选一：列证据 / 文档证据）。

### G4 · 属性 `data_type` 落进快照

**现状**：`Property` 只有 `name/column/nullable/note/sample`，**没有类型位**。
⇒ 导出时 752 项 xsd 全走 `heuristic`，**尽管文档明确给了类型**。

**要**：加 `data_type`，导出时 `rangeSource: doc`（而不是 heuristic）。

### G5 · `ontok:sourceKind` 增加第三态 `"none"`

**现状**：只有 `api` / 默认（db）。
⇒ v3 里 **9 个有数据的类和 21 个没数据的类在 TTL 里长得一模一样**。

**后果（最危险）**：数字员工查「云硬盘」得到空结果，**当成"没有云硬盘"**——正是我们反复在防的"半个答案"。

**要**：
```turtle
:cbs_disk a owl:Class ; ontok:sourceKind "none" .   # 概念存在，本部署无数据源
```
并在查询命中这类时返回 `meta.complete: false` + 说明（与 `@pending` 同一条纪律）。

## 不做

- ⛔ 不做语义自动对齐（AI 猜"Instance"= "cvm_instance" 然后自动合）——**必须人签**
- ⛔ 不改 v3 的 30 类内容
- ⛔ 不引依赖

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | G1/G2 **默认不改变现有行为**（新命令，不影响已有版本） |
| R2 | 对齐结果**必须可追溯**：每个合并的类，记"文档类 ← 实体 key"的来源 |
| R3 | G3/G4 **只增字段**，不改既有快照语义（老快照仍能读） |
| R4 | G5 只影响**导出与查询提示**，⛔ 不改 SPARQL 语义 |
| R5 | 所有新能力的判据都要能**红**（把开关关掉 ⇒ 必须失败） |

## 交付物

1. 五个能力 + 各自判据
2. **端到端**：`branch v2 → 对齐 30 个文档类 → 冻结 v4 → 导出一个本体（~35 类）→ 其中 9 类有数据可查`
3. 已知限制
