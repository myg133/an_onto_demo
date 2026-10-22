# REQ-118 · 交付补充登记（A-1 ~ A-5）

> 本文件是 Dev 侧对 BA 验收意见的**实现与登记**，不是需求文档。
> 裁定权在 BA / QA；这里只写「做了什么、没做什么、为什么」。

## A-1（已修）· P4 到库那一层

### 实现位置

| 落点 | 文件 | 说明 |
| --- | --- | --- |
| `ontok-propose --names` 写 `inputs.names` | `src/bin/ontok-propose.rs`（`names_raw` 载入之后 + 写盘之前） | `{file, sha256, agent, version, reviewed_by}`；`reviewed_by` 取翻译表 `_proposal.reviewed_by`，没有就 `null`（字段仍存在）；`_proposal` 有 `file/sha256` 时额外带 `proposal: {file, sha256}` |
| `ontok propose import` 保留其它输入键 | `src/cli.rs`（`propose_cmd` 的 `inputs` 分支） | 旧实现命中 `exclusions` 时只写 `{"exclusions": rec}`，会**吞掉 `names`**；现在写回原对象、替换 `exclusions` |
| `ontok concept import` 写 `inputs.concepts` | `src/cli.rs`（`concept_cmd` 的 import 分支） | `{file, sha256, version, declared_by, confirmed_by, reviewed_by}`；按**文件字节**算 sha256（不是解析后的字符串） |

### 真跑验证（2026-10-22，本地库 `anspire_real`）

```text
# names（96 表）
ontok-propose --names out/req118-real/cache-fresh/table_names.as-names.json --project req118-a1 ...
ontok propose import --file out/req118-real/a1-names.proposal.json
→ DB proposals.inputs.names = {
    "agent": "ai:qwen36-27B",
    "file": "out\\req118-real\\cache-fresh\\table_names.as-names.json",
    "proposal": {"file": ".../names.proposal.json", "sha256": "5d991434…"},
    "reviewed_by": null,
    "sha256": "a70f697a8aa08a0fb5aae417804d766c93dff6e3f7657e29718a2ae51444408a",
    "version": "0.1.0+ai.qwen36-27B"
  }
  文件实际 sha256 = a70f697a…08a  ✅ 一致

# names + exclude 同时给
→ DB inputs 顶层键 = ["exclusions", "names"]（两个都在）
  inputs.names.sha256      = a70f697a…08a
  inputs.exclusions.sha256 = 004b2fdd…4d   ✅ 与排除文件实际 sha 一致

# concepts
ontok concept import --file out/req118-real/groups.proposal.json --project anspire_grouped --base-version 2
→ DB proposals.inputs.concepts = {
    "confirmed_by": null,
    "declared_by": "ai:qwen36-27B",
    "file": "out\\req118-real\\groups.proposal.json",
    "reviewed_by": null,
    "sha256": "5f67e13b…cf2",
    "version": "1.0.0"
  }
  文件实际 sha256 = 5f67e13b…cf2  ✅ 一致
```

回归钉子：`tests/test_req118_provenance_inputs.rs`（3 个 DB 用例）——
names / names+exclude / concepts 各自「从库里读回来比 sha256」。
其中 `names_and_exclusions_both_survive_import` 专门钉住「`exclusions` 校验分支吞掉 `names`」这个缺口。

字段约定：`reviewed_by` / `confirmed_by` **允许为 null，但键必须存在** ——
「没人签过」与「没有这一栏」是两件事，将来签了要有地方记。

## A-2（BA 裁定不改）· 排除过宽的 9 条人工复核清单

> **不预设答案**：以下「建议」只说明支持/反对的依据，最终由业务签字。
> 产品版比对话版多排 9 张；对话版 12 条全部命中且类别相同。
> **不调提示词**的理由见 BA 裁定：对话里的隐性上下文不是标准答案。

| # | 表 | 产品版类别 | 建议 | 依据（供签字人判断） |
| --- | --- | --- | --- | --- |
| 1 | `usage_reconciliation_log` | internal_pipeline | **存疑** | 对账日志可能承载「用量差异如何被处理」的业务语义；若客户会追问对账结果，应保留。 |
| 2 | `simulate_login_sessions` | internal_pipeline | **倾向排除** | 客服/运维「模拟登录」的会话状态；除非客户可见「代客登录」审计记录。 |
| 3 | `simulate_login_operations` | internal_pipeline | **倾向排除** | 同上，操作流水属运维审计。 |
| 4 | `dim_customer_binding` | report_intermediate | **倾向保留** | `dim_` 前缀是数仓维度，但「客户渠道归属」可能是真实业务维度；对话版把它归进「客户与子账号」组。 |
| 5 | `instance_task_manager` | internal_pipeline | **存疑** | 任务配置/调度元数据；若智能体任务本身是客户可感知对象，配置也应保留。 |
| 6 | `instance_task_record` | internal_pipeline | **存疑** | 任务运行流水；若客户能查看任务历史，它是业务对象而不是内部日志。 |
| 7 | `file_parser_task` | internal_pipeline | **倾向排除** | 文件解析的技术过程状态，客户不感知。 |
| 8 | `proxy_operation_records` | internal_pipeline | **存疑** | 代理操作流水属审计；若与计费/授权变更绑定，可能要保留追溯。 |
| 9 | `licenses_archive` | backup_snapshot | **倾向保留** | 授权归档是客户授权历史的一种状态；对话版保留了它（`licenses_archive` 在基线 12 条之外）。 |

## A-3（BA 裁定）· AC-001 的判据范围

- AC-001 的六字段（`claim / evidence / confidence / reason / decided_by / reviewed_by`）
  适用于各提案文件顶层的 **`proposals[]`**。
- `concept suggest --ai` 的 **`groups[]`** 是 REQ-010 的 `ConceptGroup` 形状
  （`key / display_name / members / scope_hint / low_confidence / extracted_by / evidence`），
  **不叠加**那六栏；`concept import` 读的也是 `groups[]`，多出的 `proposals[]` 被忽略。
- 已写入 `src/extract/proposal.rs` 与 `src/extract/ai_group.rs` 的模块文档。

## A-4（登记）· `low_confidence` 真实数据 0 条

- 真实数据：name 96 条最低 0.65；exclude 96 条最低 0.70；concept 9 组最低 0.70 ——
  **没有任何一条 < 0.60**，`low_confidence: true` 在真实产物里为 **0**。
- R4 的分支由单测覆盖（`low_confidence_is_marked_not_dropped`：0.42 标 true、0.60 不标），
  集成夹具里 `_volc_dts_pg_ddl_abc` 0.50 也标了 true。
- ⛔ 结论只能写「低置信标注的分支已实现并有单测」，**不能**写「真实数据已验证低置信标注」。
- 已写入 `src/extract/proposal.rs` 模块文档「已知限制」节。

## A-5（登记）· `concept suggest --ai` 单次全量请求

- 现状：整个快照（96 实体 + 52 条关系）**一次请求**；实测输出 7,842 token、一次成功。
- 触发条件：实体数 × 描述/属性长度让模型输出超过 `max_tokens`（默认 4096）⇒
  `finish_reason = "length"` ⇒ `completion_text` 报错 ⇒ **整份回答作废**
  （不是丢几个组；该批记 `truncated`，不产出文件）。粗略阈值：实体数 > ~150–200
  或描述很长时。
- 将来修法（本轮**不做**）：两阶段「分片 + 合并」—— 先按业务域/关系连通分量分片
  （每片一次调用，指纹按片算），再用跨片关系把同域组并起来；或先让模型只给
  「组名 + 种子成员」，再逐组补齐成员。合并规则本身是另一个决策点，不在本 REQ 顺带决定。
- 已写入 `src/extract/ai_group.rs` 模块文档「已知限制」节。
