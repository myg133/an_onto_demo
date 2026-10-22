# REQ-118 · 交付补充登记（A-1 ~ A-5 / N-1 ~ N-7）

> Dev 侧对 BA 验收意见的实现与登记，不是需求文档。裁定权在 BA / QA。
> 最终产物见 `out/req118-real/README.md`（单轮口径 = `run2/`）。

## A-1（已修）· P4 到库那一层

| 落点 | 文件 | 说明 |
| --- | --- | --- |
| `ontok-propose --names` 写 `inputs.names` | `src/bin/ontok-propose.rs`（`names_raw` 载入后 + 写盘前） | `{file, sha256, agent, version, reviewed_by}`；`reviewed_by` 取翻译表 `_proposal.reviewed_by`，没有就 `null`（键仍存在）；`_proposal` 有 `file/sha256` 时额外带 `proposal: {file, sha256}` |
| `ontok propose import` 保留其它输入键 | `src/cli.rs`（`propose_cmd` 的 `inputs` 分支） | 旧实现命中 `exclusions` 时只写 `{"exclusions": rec}`，会**吞掉 `names`**；现在写回原对象、只替换 `exclusions` |
| `ontok concept import` 写 `inputs.concepts` | `src/cli.rs`（`concept_cmd` 的 import 分支） | `{file, sha256, version, declared_by, confirmed_by, reviewed_by}`；按**文件字节**算 sha256 |

真跑（本地库 `anspire_real`，`psycopg2` 直读 `proposals.inputs`；最终口径 run2）：

```text
# names + exclude 同给：proposal 8ec790bc-a24a-43ed-8639-e205714ee2dc
top_keys = ['exclusions', 'names']          ← 两个块都活下来
inputs.names.sha256      = c64179f961cd083be9f422b02ac01ec6fe55c536aea29ffeb07dde7d5ace24de
文件实际 sha256          = c64179f9…                ✅ 一致
inputs.names.agent       = ai:qwen36-27B
inputs.names.version     = 0.1.0+ai.qwen36-27B
inputs.names.reviewed_by = null                     （键存在）
inputs.names.proposal    = {file: out\\req118-real\\run2\\names.proposal.json,
                            sha256: c2a45ac19a72005fc9e255639ddd3027297d0ef9f155062095ca15e74db8a3cb}
inputs.exclusions.sha256 = 80f3eef26dad80b6df8564875d6723076430b9a772755ef855d25f4a5c0a9e74
文件实际 sha256          = 80f3eef2…                ✅ 一致

# concepts：proposal a9ef8fb0-a8b7-47b6-bac4-15ced223c911
inputs.concepts = {file: out\\req118-real\\run2\\groups.proposal.json,
                   sha256: b98f3025d82c67e93dc29ea1b29fb5adbafd49f3308c31241813d46c9eff7f73,
                   version: 1.0.0, declared_by: ai:qwen36-27B,
                   confirmed_by: null, reviewed_by: null}
文件实际 sha256 = b98f3025…5f73                ✅ 一致
```

字段约定：`reviewed_by` / `confirmed_by` **允许 null、键必须存在**。
回归钉子：`tests/test_req118_provenance_inputs.rs`（3 个 DB 用例；其中
`names_and_exclusions_both_survive_import` 钉住「exclusions 分支吞掉 names」）。

## A-2（BA 裁定不改）· 排除过宽的 9 条人工复核清单

> 不预设答案；「建议」只说明支持/反对的依据，最终由业务签字。
> 产品版比对话版多排 9 张；对话版 12 条全部命中且类别相同。

| # | 表 | 产品类别 | 建议 | 依据 |
| --- | --- | --- | --- | --- |
| 1 | `usage_reconciliation_log` | internal_pipeline | **存疑** | 对账日志可能承载「用量差异如何处理」的业务语义 |
| 2 | `simulate_login_sessions` | internal_pipeline | **倾向排除** | 客服/运维模拟登录的会话状态 |
| 3 | `simulate_login_operations` | internal_pipeline | **倾向排除** | 同上，操作审计 |
| 4 | `dim_customer_binding` | report_intermediate | **倾向保留** | 数仓维度，但客户渠道归属可能是真业务维度（对话版归入客户组） |
| 5 | `instance_task_manager` | internal_pipeline | **存疑** | 任务配置；智能体任务若是客户可感知对象则应保留 |
| 6 | `instance_task_record` | internal_pipeline | **存疑** | 任务流水；客户能看任务历史则属业务 |
| 7 | `file_parser_task` | internal_pipeline | **倾向排除** | 文件解析技术状态，客户不感知 |
| 8 | `proxy_operation_records` | internal_pipeline | **存疑** | 审计流水；若绑定计费/授权变更需保留追溯 |
| 9 | `licenses_archive` | backup_snapshot | **倾向保留** | 授权归档是客户授权历史状态（基线未排除） |

## A-3（BA 裁定）· AC-001 的判据范围

- 六字段（`claim / evidence / confidence / reason / decided_by / reviewed_by`）
  只适用于各提案文件顶层的 **`proposals[]`**。
- `concept suggest --ai` 的 **`groups[]`** 是 REQ-010 的 `ConceptGroup` 形状
  （`key / display_name / members / scope_hint / low_confidence / extracted_by / evidence`），
  **不叠加**六栏；`concept import` 读的也是 `groups[]`。
- 已写入 `src/extract/proposal.rs` 与 `src/extract/ai_group.rs` 模块文档。

## A-4 / N-7（登记，改实测值）

- **最终口径 `run2/`**：name 最低 **0.70**、exclude 最低 **0.60**、concept 最低 **0.70**；
  `< 0.60` 的条目 **0 条**。
- **历史轮 `run1/`**：name 最低 **0.60**、exclude 24 条最低 0.70。
- **跨两轮的最低值 = name 0.60 / exclude 0.60**（与 QA 实测的 0.60/0.60 一致；
  QA 读到 0.60/0.60 正是因为目录里混着 run1 —— N-3 已清理为分轮目录）。
- R4 分支仅由单测（0.42 → true、0.60 → false）与夹具（0.50）覆盖；
  ⛔ **不能**写「真实数据已验证低置信标注」。
- 已写入 `src/extract/proposal.rs`「已知限制」。

## A-5 / N-1（已修）· 失败批次的实际行为

`run_batches` 把「超时 / 无 usage / 截断 / 整份解析失败」记进 `failed_batches`，
由 `Completeness` 三态决定结局：

| 情形 | 文件 | 退出码 | 文件里的字段 / 提示 |
| --- | --- | --- | --- |
| 全成功 | 写 | **0** | `stats.complete = true`、`failed_batches = 0` |
| 部分失败 | 写（可留） | **≠0** | `complete = false`、`failed_batches = N ≥ 1`；报「不完整，⛔ 不要直接导入」 |
| 全部失败 | **不写**（一个字节都不落） | **≠0** | 报「全部 N 批调用都失败 —— 未产出任何提案文件」；无「可直接 import」 |

- 全失败不产文件是刻意的：`proposals: []` 的 JSON 完全合法，导入方会当成
  「真的、恰好 0 条」—— 与「`.pdf` 静默跳过」同一类 fail-open。
- `name suggest` 在**部分失败**时也**不写 `--as-names`**（缺 N 批的名字表会被下游静默丢表）。
- `concept suggest --ai` 只有 **1 批**（整快照一次请求）⇒「部分失败」在它上面
  **结构上不可达**（只有全成功 / 全失败）；三态闸是共用实现，将来分片后自然生效。
- 已写入 `src/extract/ai_group.rs`「已知限制」（与实际行为一致）。

## N-2（已修）· comparison.md 96 行

`out/req118-real/run2/comparison.md` 的 name 段现在是 **96 行**：
一致 **9** + 名字不同 **74** + 产品版新增 **13** + 对话版独有 **0** = **96**。
（上一版生成脚本漏了「一致」分支的行，只有 87 行 —— 已修。）

## N-3（已修）· 交付目录单轮口径

```
out/req118-real/
├── README.md        ← 哪轮是最终口径 + 各轮数字
├── run2/            ← ★ 最终口径（complete: true；名字 96 / 排除 21 / 分组 9）
├── run1/            ← 历史冷跑（只作 N-4 差异与 0.60 最低值证据；缺 N-1 字段）
├── n1/              ← N-1 三种情形（partial 文件 + all-fail 空缓存）
├── budget/          ← --budget-tokens 1（无 proposal 文件）
├── provenance/      ← A-1 真跑 + 读库脚本
├── p2/              ← AC-008 本体证据
├── smoke/           ← 4 表冒烟
├── logs/            ← 运行/测试日志
└── help-*.txt
```

顶层只有子目录，没有混轮次的文件；`comparison.md` 只在 `run2/`（最终口径）。

## N-4（登记）· 冷跑不复现

- **同缓存重跑**：三命令均 **0 token + 逐字节相同**（AC-005 / ADR-005 P3 指的就是这个）。
- **冷跑（无缓存）不保证相同**：同一输入两次独立调用，模型采样不同 ——
  实测排除 **21 条（run2）vs 24 条（run1）**，名字最低置信度 0.70 vs 0.60。
- ⛔ 不得写成「永远可复现」。P3 的可复现前提是**指纹缓存**。

## P2 旁路（登记为已知缺口）

- `ProposalItem.reviewed_by` 是私有字段，生产路径只有 `finalize_item` 能构造（恒 `None`）；
  `render_document` 恒写 `null`。
- **缺口**：`derive(Deserialize)` 仍能把一份 JSON 反序列化成 `Some(...)`。
  当前**没有任何生产代码反序列化 `ProposalItem`**，所以不构成路径；
  但**没有测试钉住「不可写」** —— 给结构体加一个 pub setter 后，
  355（lib）+ 13（req118，含 N-1 两条）+ 3（provenance）条测试仍全绿（QA 实测时是 351 + 11 + 3；
  本轮 N-1 又加了 2 条 lib 测试，故为 353）。
  若将来出现读回路径，必须补一条「签名只能由评审流程写入」的判据。

