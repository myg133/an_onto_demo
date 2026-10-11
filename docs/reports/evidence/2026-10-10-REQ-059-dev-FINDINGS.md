# REQ-059 · 实测记录（边做边落盘）

工作区：`/home/myg133/Projects/Anchnet/Ontology/feature-REQ-059`
分支：`feature/REQ-059`（基线 `80464a9`）
本机自编译 target（**无** `ln -s`，**无** `CARGO_TARGET_DIR`，**无** `--release`）

---

## 0 · 基线复现（未改任何代码）

命令（不带 `--agent`，输入是指定 fixture）：

```bash
./target/debug/ontok-propose \
  --schema /home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json \
  --out   .review-workspace/req059/base-default/proposal.json \
  --stats .review-workspace/req059/base-default/stats.json \
  --project extracted
```

退出码 `0`。首行输出：`翻译表      : mcode-agent (MiniMax Code) v1.2.0`

产物 `extracted_by`：

```json
{
  "agent": "mcode-agent",
  "version": "1.2.0",
  "round": 1,
  "failed_groups": [],
  "source": "introspect 2026-10-05T04:25:53.403718151+00:00 schema=public read_only=on"
}
```

**sha 与 AC-005 基线逐字节一致**（口径：`sha256sum <file>`）：

```
cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba  base-default/stats.json
8dbd32db4f97e897152cda7b580b6c382f320d2506ad3f59642f2fb1db3e4c3e  base-default/proposal.json
```

`items` = 171 条（实体 91 · 关系 80）。

---

## 1 · 缺陷实证：比 AC 描述的**更靠上游**

### 1.1 `extracted_by.version` 是**翻译表的版本**，不是执行者的版本

基线里有：

```rust
if agent_version == "unknown" {
    agent_version = names.version.clone();   // ← 翻译表版本被当成抽取 agent 版本
}
```

实测 `extracted_by.version == "1.2.0"`，而 `src/extract/table_names.json` 的
`version` 正是 `1.2.0`。⇒ **归属字段的 agent 名与版本，两处都来自「调用方随手填的
默认值」，没有一处是真实执行者的事实。** AC 只点名了 `agent`，
`version` 是同一个病的第二处（AC-005 的 sha 变化主要来自它）。

### 1.2 `identity_cross_check` 在默认路径上**恒真**（结构上不可能触发）

| 位置 | 内容 |
| --- | --- |
| `src/agent/factory.rs` inproc 分支 | `RuleBasedAgent::new(AgentIdentity::from_registration(registration, round), names)` |
| `src/agent/rule_based.rs` `impl ExtractionAgent` | `fn identity(&self) -> &AgentIdentity { &self.identity }` |
| `src/agent/registry.rs` `from_registration` | `agent: reg.agent.clone(), version: reg.version.clone()` |
| `src/bin/ontok-propose.rs` 调用点 | `identity_cross_check(&AgentIdentity::from_registration(&registration, round), agent.identity())` |

⇒ 比较的两边**同源**。**`inproc` 路径上这个核对永远返回 `None`。**

### 1.3 AC-002 指定的那个判别输入，基线**不红**

```bash
./target/debug/ontok-propose --schema .../schema.json \
  --out .../base-fake/proposal.json --stats .../base-fake/stats.json \
  --project extracted --agent 虚构名
```

结果：**退出码 0**，stderr 无任何身份相关告警，`extracted_by.agent == "虚构名"`。

⇒ BA 描述的缺陷（「只 `eprintln!` 警告、不报错」）**在这条路径上根本没机会发生**
—— 那个告警分支在默认路径上不可达。真正的问题是**默认值本身就是编造的**，
而核对拿编造值跟自己比，比出了一个「一致」。

**这就是本条 AC 的定位偏差（详见交付报告）**：AC-002 把修法指向
「把 `identity_cross_check` 的处置从警告改成报错」，但那条路**改不动** ——
在 `inproc` 路径上根本没有第二个独立来源可比。必须先改「登记进去的是什么」。

---

## 2 · 改完之后（实跑证据）

### AC-001 · 默认路径

```
$ ./target/debug/ontok-propose --schema … --out … --stats … --project extracted
EXIT=0
  翻译表来源   : agent=mcode-agent (MiniMax Code) v1.2.0（知识资产：**谁产的这份翻译表**，不是抽取执行者）
  抽取执行者   : agent=inproc:ontok::agent::RuleBasedAgent v0.1.0（协议 inproc）—— **这一行才是 extracted_by 记的**
```

产物：`extracted_by.agent = 'inproc:ontok::agent::RuleBasedAgent'`、
`extracted_by.version = '0.1.0'`（逐字断言，非「非空」）。

带 `--agent inproc:… --agent-version 0.1.0`（与事实相符）⇒ 退出 0、逐字记下同样两个值。

### AC-002 · 身份对不上必须红

```
$ ./target/debug/ontok-propose … --agent 虚构名
EXIT=1
Error: 拒绝出产物：--agent 声明的执行者是 "虚构名"，而这一轮**真正跑起来的实现**是
       "inproc:ontok::agent::RuleBasedAgent"。…
产物存在？ proposal=NO  stats=NO
```

`--agent-version 9.9.9` 同样 `EXIT=1`，且**不留半个产物**。

### AC-003 · 混名必须被抓住

| 翻译表产出方 | 结果 |
| --- | --- |
| `inproc:ontok::agent::RuleBasedAgent`（完全同名） | EXIT=1 |
| `inproc`（产出方名是执行者名的**前缀**） | EXIT=1 |
| `inproc:ontok::agent::RuleBasedAgent (MiniMax Code)`（执行者名是产出方名的前缀） | EXIT=1 |
| `table-name-translator`（对照，不撞） | EXIT=0 |

第三行就是**实测出事的那一种**的同构形状：基线上翻译表产出方叫
`"mcode-agent (MiniMax Code)"`、`extracted_by.agent` 叫 `"mcode-agent"` ——
**并不相等**，却读起来是同一个名字。

---

## 3 · 产物 sha（**变了**，AC-005 要求的主动报备）

口径：`sha256sum <file>`；输入 `.review-workspace/fix-props/schema.json`；
命令为 `ontok-propose`（`--stats` 自带路径、不传 `--names`）、`--project extracted`。

| 文件 | 基线 | 改后 | 结论 |
| --- | --- | --- | --- |
| `stats.json` | `cda6a9a2…3cd7ba` | `cda6a9a2…3cd7ba` | **不变**（`stats` 不记归属） |
| `proposal.json` | `8dbd32db…4c3e` | `1eef9541…c5635` | **变**（归属两栏） |

**逐字段差异（口径：python json 深度遍历两份产物，全量列出）**：

```
CHANGED /extracted_by/agent   : "mcode-agent"  -> "inproc:ontok::agent::RuleBasedAgent"
CHANGED /extracted_by/version : "1.2.0"        -> "0.1.0"
```

**只有这两个字段**。`items`（171 条）、`round`、`extracted_by.source`、
`extracted_by.failed_groups` 一个字节都没动。

**为什么变**：这**就是本需求要的**（AC-001：「默认身份不许是编造占位」）。
`mcode-agent` 是 CLI 的硬编码占位，`1.2.0` 是**翻译表的版本号** ——
两者都不是「这一轮真正跑的是谁」的证据。改后记的是编译进二进制的那个实现
自报的身份与本 crate 版本。

⇒ **需要知会 BA 同步所有引用 `8dbd32db…` 这个 sha 的文档**；
`cda6a9a2…` 仍然有效。