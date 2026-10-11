# 其他测试替身是否被波及

`git diff 563f26f 40b755d -- tests/` 仅 3 个文件，逐个核完：

## tests/test_agent_rebuild.rs
- 改：`IncompleteRebuildAgent.conflict_link` 的 evidence（Vec::new() → 3 件 JSON）
- 新增：`EvidenceLessLinkAgent`
- 新增 4 条测试：`test_rebuild_contract_guard_rejects_a_group_before_it_reaches_the_proposal`、
  `..._catches_a_link_without_evidence`、`..._catches_a_property_without_nullable`、
  `test_the_contract_guard_does_not_touch_the_rule_pipeline`
- **未动**：`ConflictingAgent`(L892)、其余替身
- 注意：新 4 条测试里 3 条复用 `IncompleteRebuildAgent`，都显式传 `conflict_link: false`
  ⇒ 与被改的 fixture 无耦合 ✓

## tests/test_agent_orchestration.rs
- 改：`len()==1` 断言 → `len()==2`（REQ-024 加了 acp 分派，**如实更新**，属必要修正）
- 新增：`test_factory_refuses_to_build_an_acp_agent_without_a_usable_endpoint`
  —— 两例（`None` / `"   "`）都在 `factory.rs:94-98` 的
  `map(str::trim).filter(|e| !e.is_empty())` 处就 bail，**不会拉起子进程** ✓ 安全
- **无其他替身改动**

## tests/test_acp_extraction.rs
- 新增 3 条：`capabilities_is_a_pure_function_that_reads_no_field`（**源码级**判据）、
  `capabilities_are_static_and_never_depend_on_the_answer`（行为级）、
  `a_conforming_answer_does_not_make_the_adapter_declare_the_capability`
- **未改任何既有替身**（`ScriptedTransport` 等原样）
- ⚠️ 脆弱性（维护性，非缺陷）：`capabilities_is_a_pure_function_that_reads_no_field`
  用**字面串**定位函数体（`find("fn capabilities(&self) -> Vec<String> {")` +
  `find("\n    }")`）⇒ rustfmt 改缩进或改签名即误报红。这是该仓既有做法
  （`test_agent_modules_have_no_direct_db_access` 同样读源码），**取舍可接受**，但要记在案。

## 结论
**除 `IncompleteRebuildAgent` 外，没有别的替身被本批改动波及。**
