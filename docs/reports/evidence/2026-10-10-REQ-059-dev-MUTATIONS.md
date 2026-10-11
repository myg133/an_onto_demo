# REQ-059 · 变异自证记录（判别力）

命令口径：`cargo test --test test_extracted_by_provenance`（本需求新增用例文件）
每个变异单独改一处 → 编译 → 跑 → `git checkout --` 还原。
**编译失败不算变异执行**（M2 第一次尝试就是编译失败，已重做成 M2'）。

| 变异 | 改了什么 | 结果 | 存活？ |
| --- | --- | --- | --- |
| **M1** | 把默认路径的身份改回硬编码占位（`agent: "mcode-agent"` / `version: "1.2.0"`）= 模拟「REQ-059 没做」 | **5 条红**：`test_default_path_records_the_real_executor_not_a_placeholder`、`test_declared_agent_that_matches_the_executor_is_recorded_verbatim`、`test_fabricated_agent_name_with_inproc_must_be_rejected`、`test_provenance_change_does_not_alter_the_extracted_content`、`test_cli_refuses_to_emit_when_executor_name_collides_with_table_producer` | 否 |
| **M2'** | `require_declared_matches_actual` 从 `bail!` 降级为「只 `eprintln` 告警、照常出产物」（AC-002 明令禁止的处置） | **4 条红**：两条 fabricated 用例 + declared 用例 + 内容不变用例 | 否 |
| **M3** | `executor_name_collision` 去掉「互为前缀」判定，只留「完全相同」 | **2 条红**：纯函数用例 + 真实二进制端到端用例 | 否 |

**存活变异：0 条。** 三条 AC 各有至少一条判据在变异下变红。

⚠️ M1 有一条用例**没红**：`test_fabricated_agent_version_must_be_rejected`
—— 因为 M1 把身份改回占位后，`--agent-version 9.9.9` 仍然与 `"1.2.0"` 不符，
照样被拦下。这不是漏网，是该用例判的是「版本断言生效」而不是「默认身份是谁」；
它的判别力由 M2' 证明（把拦截降级为告警时它立刻红）。

⚠️ M3 有一条用例**没红**：M3 只删了前缀判定，「完全同名」那一支还在，
所以 `test_translation_table_and_executor_names_must_be_distinguishable` 里的
「完全同名」子项本就不该红；整条用例红在**前缀**那两项上 —— 那正是实测出事
的形状（翻译表产出方 `"mcode-agent (MiniMax Code)"` vs 执行者 `"mcode-agent"`，
两者**并不相等**）。

---

## 一次**假通过**，值得记下来

REQ-059 改完之后 `test_the_production_entry_still_reports_the_identity_fork`
**仍然是绿的** —— 而它的断言正是「生产入口不许再有 `eprintln!("⚠ {warn}")`」。

根因：那条断言用的是 `src.contains("⚠ {warn}")`，而我新写的**注释**里恰好含有
这段字面量。**散文里写着 ≠ 代码里还在。**

处置：注释改成不复述那段字面量（散文改写），并把断言从「必须打出来」翻成
「必须拦住 + 不许再出现那段字面量」。以后改这一段的人不会踩同一个坑。