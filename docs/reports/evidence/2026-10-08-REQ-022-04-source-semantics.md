# 二、`source` 字段语义与 ADR-003 合规性

## 1. 两个 `source`：确系同名不同义（代码 + 双向文档确认）

| | 提案侧 `ProposalProvenance::source` | 快照侧 `snapshot::ExtractedBy::source` |
|---|---|---|
| 类型 | `Option<String>` | `Option<ProvenanceSource>`（枚举） |
| 定义 | `src/extract/propose.rs`（REQ-002 契约字段） | `src/domain/snapshot.rs:79`（落库那份） |
| 含义 | **勘查留痕**：`introspect <时刻> schema=<schema> read_only=<on/off>` | **来源通道**：`tool_produced` / `declared` |
| 证明的是 | 这批 schema 何时、从哪个 schema、只读事务里读出来的 | 这个溯源值是本工具跑的，还是从外部 JSON 读进来的 |
| 持久化 | 进 `proposal.json` | 进 `proposals.extracted_by` 与 `ontology_versions.extracted_by` |

**双向交叉引用均在位**：
- `propose.rs:22-41`（提案侧改名理由 + 「`source` 在两侧**含义不同** —— 别混」对照表）
- `snapshot.rs:66-72`（「⚠️ 与 `ProposalProvenance` 不是同一个类型」+ 指回提案侧）
- `cli.rs:440-452`（import 处第三张对照表：字段名相同、类型不同）

**防错位是真的**：import 已从「整块 `serde_json::from_value::<ExtractedBy>`」
改为**逐字段取**。实测验证（§2）—— 我故意让导入 JSON 的 `source` 装的是勘查留痕
字符串（`introspect 1999-01-01...`），库里写进去的仍是枚举 `declared`，
**字符串没有顶替枚举**。

## 2. 我自己跑的一次 import —— 库里原始列（非程序自述）

输入（伪造值，照 ADR-003 原始剧本）：`agent=totally-not-a-real-agent`、
`version=9.9.9-fake`、`round=7`、`failed_groups=["payments","risk"]`、
`source="introspect 1999-01-01T00:00:00Z schema=public read_only=on"`。

CLI 结果（exit 0，静默入库 —— ADR-003 的问题本身未被"修掉"，这是 D2 的定位）：
```
提案 p1 已导入，共 1 个条目：
  [1] add_entity_type 设备
  （抽取来源：totally-not-a-real-agent 9.9.9-fake）
```

**库里 `proposals.extracted_by` 原始列**：
```json
{"agent":"totally-not-a-real-agent","failed_groups":["payments","risk"],
 "round":7,"source":"declared","version":"9.9.9-fake"}
```
⇒ `source` **存在**，值 = **`declared`** ✔
⇒ 勘查留痕字符串**没有**被当成来源通道写进去 ✔（防错位生效）

再 `review decide --accept-all` + `proposal apply` 后，
`ontology_versions v2.extracted_by` 同一份 JSON，`source` 仍是 `declared`
⇒ **留痕能穿过 apply 传染到版本表**（原始抱怨正是"一路传染"；现在传染时**带着标签**）。

### 全库分布（只读 SELECT）
```
proposals (9912 行)              ontology_versions (25702 行)
  extracted_by IS NULL   : 8845    source 为 NULL/无键 : 25702
  有 eb 但无 source 键    : 1069    source='declared'   : 1
  source='declared'      :    6    source='tool_produced': 0
  source='tool_produced' :    0
```
全库仅 7 条 `declared`：6 条来自 Dev 测试项目（`t_provsrc_*`），第 7 条是我造的。
1069 条「有 eb 但无 source 键」= REQ-022 之前的历史数据，按 ADR-001 记 `None`（不猜）。

## 3. `source` 由「谁在写」决定，不由「被写的内容」决定 —— 符合 ADR-003 原话

ADR-003：「**由写入通道负责留痕**」。核实：
- 全仓 `snapshot::ExtractedBy` 的**生产写入点只有 2 处**：
  `src/cli.rs:322`（concept import）与 `src/cli.rs:510`（propose import），
  **两处都无条件标 `Declared`**。
- 取值**不看** `agent`/`version`/`failed_groups` 的内容 —— 我伪造的
  `9.9.9-fake` 与真实值走同一分支，结论一致。
- `db/snapshot_store.rs:209,343` 是**读**路径，不是写入方。
⇒ **取值确由写入通道决定** ✔

## 4. `ToolProduced` 无写入方 —— Dev 自报属实，我的独立判定

穷举 `ToolProduced|tool_produced|ProvenanceSource::|is_declared` 全仓命中：
- `snapshot.rs:55` 枚举定义本身
- `snapshot.rs:40` 文档表格里的一行
- `tests/test_provenance_source.rs:190,195,204` 一条**反向**断言
- 生产代码 `ProvenanceSource::Declared` 只有 `cli.rs:308`、`cli.rs:497` 两处写

**生产代码零处写 `ToolProduced`**；数据库 `tool_produced` 计数 = 0（两表皆然）。

### 判定：**不是违规空壳，但当前判别力为零 —— 有条件放行的残留缺口**

不判为阻断：
1. **方向保守**：所有行标「未经验证的外部声明」，没有任何一行被**高估**成可信；
   `None` 明确不猜成自产。若方向相反（猜成自产）才是安全问题。
2. **ADR-003 自身定位**为留痕而非验证（D2「不是安全漏洞，是数据可信度问题」），
   (c) 签名凭证已推迟，且把 `failed_groups` 列为**下一批**优先级。
3. 写入通道确实**没有**能力区分「这份 JSON 是 ontok-propose 刚写的还是手写的」——
   `import` 拿到的永远是一个文件，标 `declared` 对该通道而言**如实**。

诚实边界（须写进验收记录）：
- ADR-003 (b) 字面要求「**区分**『工具自产』与『外部声明』」，现在只落地**一半**：
  枚举两个取值，可判别的那一半不可达。⇒ `is_declared()` 对库中任何由
  import 写入的行**恒为 true，零判别力**。
- **「从候选重建」跑出来的东西 `source` 会是什么？**
  `ontok-propose`（重建路径）**不连库**，只写 `proposal.json`，其
  `extracted_by.source` 是**勘查留痕字符串**（`assemble.rs:189`）。
  该 JSON 进库**必须**再走 `ontok propose import`，于是**一律落成 `declared`**。
  ⇒ **重建路径产出的溯源，交付后库里没有一行会标成 `tool_produced`。**
- 后续项（建议记 REQ-023 / backlog，**不在本批阻断范围**）：
  ① `ontok-propose` 直连库时能标 `ToolProduced`（或 import 侧接受「本进程自产」握手凭据）；
  ② 给 `source` 配查询面（`is_declared` 目前只有测试在用，生产无调用点）；
  ③ 按 ADR-003 优先级单独立项处理 `failed_groups`。

## 5. 顺带发现的一处**行为变更**（import 侧，非 propose.rs，供记录）
`cli.rs:466-482` 让 `failed_groups` 额外接受**结构化对象**形状（取 `group_id`）。
改动前：对象数组 ⇒ 反序列化失败 ⇒ import 报错；改动后：静默接受并转成字符串数组。
方向合理（否则"工具自己产的提案导不进去"，ADR-003 点名的危险字段反而只能手写），
且仍标 `declared` 可核对。但这是 `import` 通道**唯一**一处行为放宽，
而 ADR-003 恰点名 `failed_groups` 是「真正危险的字段」—— 建议验收记录留一笔。
