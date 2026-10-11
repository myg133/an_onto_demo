# QA REQ-020a — 缺口 1（pub 旁路收口）验证记录
时间：2026-10-08 11:2x  worktree: feature-REQ-020 @ e5717a8

## 1. 红线确实调用 propose::extract —— 属实
| 文件 | import | 实际调用行 | 调用形态 |
|---|---|---|---|
| tests/drop_rate.rs | :26 `use ontok::extract::propose::extract;` | :95 | `extract(&report, &names, "dropprobe","dropprobe","dropprobe")` |
| tests/test_fk_naming.rs | :42 `use ontok::extract::propose::extract;` | :108 | `extract(&schema, &names, "fk_fix","test","test")` |

两文件均在 tests/ 下 = 独立 crate（integration test），从 crate 外调 pub 符号。
→ Dev「无法收窄 extract」的理由成立：收窄 `extract` 会直接编译失败打破禁改红线。

## 2. 可见性声明现状（与 Dev 声称一致）
- src/extract/propose.rs:215 `pub fn extract(`                → 仍 pub（红线需要）
- src/extract/propose.rs:280 `pub(crate) fn extract_with_context(` → 已收窄 ✅
- src/agent/rule_based.rs:122-123 `#[cfg(test)] pub(crate) fn propose(` → 已收窄 ✅

## 3. 两条断言迁移前后逐条对照
### 3.1 test_rule_based_agent_output_is_byte_identical_to_legacy_extract
迁移前 tests/test_extraction_agent.rs → 迁移后 src/agent/rule_based.rs:mod tests
断言体**逐条一致**（仅文档注释改写）：
- serde_json::to_string_pretty(&legacy) == to_string_pretty(&via_agent) 字节比对 ✅ 保留
- round == 1 ✅ / extracted_by.agent == "mcode-agent" ✅ / version == "2026.10.07" ✅
- extracted_by.round == 1 ✅ / failed_groups.is_empty() ✅
### 3.2 test_extraction_stats_fields_are_untouched
断言体**逐条一致**：10 个字段名排序后 == expected。✅ 保留
（11 个 expected 项 = 10 标量 + 1 列表，原文注释「10 个标量 + 1 个列表」）

### 判据强度独立判定：**未变弱**
理由：
(a) 两条断言都是零 I/O 纯函数（内联 schema()/names()，不读文件不连库）→ 换到 crate 内不引入外部依赖。
(b) 被断言的对象（ExtractionOutcome / ExtractionStats）没有可见性/位置变化 → 断的还是同一条关系。
(c) 迁移后**多出一条**判据（Dev 声称的「生产构建里该方法不存在」）→ 净增量非减量。
(d) 唯一理论风险「搬进 mod tests 获得 cfg(test) 可见性 → 判据变松」不成立：
    断言调用的是 `extract`（propose.rs:215，仍 pub，任何时候可见），
    不是任何 cfg(test) 专属符号。mod tests 给予的是"能调到 agent.propose"，
    而原断言本来就调的 agent.propose —— 判定对象未变。
(e) tests/test_extraction_agent.rs 未留悬空 import：extract 仍在 :325/:517 使用，
    RuleBasedAgent 仍在 :204 使用 → 无 unused import 警告。
