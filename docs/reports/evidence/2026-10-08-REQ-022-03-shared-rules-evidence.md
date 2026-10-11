# 两条路径是否真共用同一份规则 —— path:line 证据

全仓穷举（排除测试）后，每个被抽出的函数**恰好 2 个调用点**，无副本：

| 函数 | 全量路径（`extract_with_context`） | 阶段 1 投影（`RuleBasedAgent::extract_group`） | 判定 |
|---|---|---|---|
| `build_properties` | `src/extract/propose.rs:402` | `src/agent/rule_based.rs:222` | ✅ 真共用 |
| `links_for_table` | `src/extract/propose.rs:428` | `src/agent/rule_based.rs:277` | ✅ 真共用 |
| `entity_display_name` | `src/extract/propose.rs:368` | `src/agent/rule_based.rs:276` | ✅ 真共用 |
| `introspect_provenance` | `src/extract/propose.rs:341` | `src/agent/assemble.rs:189` | ✅ 真共用 |

**没有「旧的还在、新的是副本」**：旧的内联代码块在 BASE 里位于
`extract_with_context` 的循环体内（BASE:391-509），HEAD 里该循环体已被替换成
4 行函数调用（HEAD:402/419/428），旧实现不存在于任何位置。

## `origin` 定义只留一份 —— 核实通过
- 唯一定义：`src/extract/propose.rs:555` `pub enum PropertyOrigin`
- `src/agent/group.rs:307` = `pub use crate::extract::propose::PropertyOrigin;`（**纯再导出**）
- `src/agent/mod.rs:77` 从 `group` 再导出一次（仍是同一个类型）
- 全仓 `enum PropertyOrigin` 只有 1 处；`grep -rl PropertyOrigin` 命中 3 个文件，无第二份定义。

## ⚠️ 边界的诚实说明（不是缺陷，但必须说清）
「一处规则两处调用」这个保证的**实际范围**是：**内置规则 agent 的两条产出路径**
（全量路径 / 阶段 1 投影）走同一份规则；重建路径（`assemble::rebuild_outcome`）
**不自己调** `build_properties`/`links_for_table`，它消费阶段 1 产出的
`CandidateType`/`CandidateLink`，并按候选携带的 `origin` 还原统计
（`assemble.rs:137-151`）。

这是设计意图（「阶段 1 契约只是**如实转述**抽取层的判断」，`propose.rs:546-551`），
不是夹带。但后果要说清：**外置 agent（REQ-023 协议 adapter）若自己编候选**，
其 `origin`/`display_name` 不经过共享函数，重建路径会**忠实照抄**。
所以保证是「规则 agent 内部一份规则」，不是「所有 agent 一份规则」。
