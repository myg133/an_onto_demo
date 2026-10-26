# 状态：REQ-154 · 契约可维护性四小修

## 当前状态

**已完成**（2026-10-26 · 合并 `aed51c6`）

## 收口

| 项 | 结果 |
| --- | --- |
| Dev | `4070a73` |
| 合并 | `aed51c6` |
| 8-case 对拍（**BA 自己做**） | 04 979→**1000** · 05 893→**1074** · 其余 6 个**逐字节不变** |
| 字节 vs 字符 | Dev 报 hint +105 **字符**，实测 +181 **字节**（中文 3 字节/字）——两者都对 |
| `get` 键数 | v1 仍 4 / v2 仍 10（未动） |
| 禁改面 | `Cargo.toml`/`lock`、`tools.rs`、`property_filter.rs` 均未动 ✅ |
| 清理 | worktree + 22.97 GB 编译缓存已删 |

### ⚠️ QA 未做（用户决定直接合并）

按规程应先 QA。本次**跳过**的依据：BA 自己的 8-case 逐字节对拍 + Dev 的变异自证
（`meta` 写死 1 ⇒ v2 断言红；示例改回旧名 ⇒ help 断言红）。**如需可随时补定向 QA。**

## 遗留（未做）

| # | 项 | 归属 |
| --- | --- | --- |
| 1 | `docs/RECIPE.md:33` 仍教「见 4 键 = 没升 v2」 | code |
| 2 | `skills/ontok-query/SKILL.md:254` 仍写 `ontok access fetch` | workspace |
| 3 | `Deploy/digital-employee/AGENTS.md:183` 同上 | Deploy |
| 4 | AUTH-SPEC §4/§6 的「结尾不加 `\n`」「`\` 折行不能照抄」 | code |
| — | **RECIPE.md 归属未定**：用户认为它**业务强相关**，不该放在通用 skill 里 | 待议 |
