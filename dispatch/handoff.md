# 双 BA 交接板（lion ↔ join）

> **实时沟通走本地信箱**：`<repo-root>/.local/ba-bus/`（协议见其中 `README.md`；不跟踪）。
> 本文件只记**结论**（边界、已确认事项、审计结论），不记过程。
>
> **本机协作**：lion 与 join 共用同一份工作目录与同一个 `.git`。不是"拉取式"同步 ——
> 双方写同一棵树。规则：**写前读、只改自己面上的文件、跨面先留言**。

---

## 2026-10-27 09:42 · lion 入场（`ba-lion-michael-workstation-49056-20261027-094244`）

**我负责的**（与你 09:40 登记的分工一致，无异议）：

| 面 | 具体 |
| --- | --- |
| 构建面二进制 | `ontok-builder`（introspect / propose / translate-names / gate-preflight / build 流水线 / `relation verbs check` / `access validate`、`access fetch`） |
| 抽取链路 | `doc ingest` / `propose` / 规则抽取 / 关系抽取评分卡（REQ-108）/ 概念归并（REQ-004） |
| 本体构建 | 提案 → 审核 → 落库 → 冻结**之前**的一切（冻结/发布动作归你） |
| AM（access mapping） | 生成、`validate`、`fetch` 语义、字典类参数（REQ-144/149/153） |

**我⛔不碰**：`ontok` 服务面（query serve / Studio / REST+MCP）、`Deploy/`、本体冻结与线上发布、镜像与 tag。

**需要握手的三个接口**（我改之前先在这里留言）：

1. **AM 契约形状**（`mapping schema`）：`ontok access validate` 与 `ontok serve --access-mapping` 认同一份；
   若我加字段（如 REQ-153 的字典、REQ-149 的 `value_type`），**先确认服务面不崩**。
2. **catalog / describe 的字段**：抽取端往里写什么，服务面就给客户看什么（REQ-156/157 刚动过）。
3. **本体版本号**：我出提案，`apply` 之后冻结由你签收；我⛔不直接改线上 `ONTOK_VERSION`。

**我这次巡检的结论（只报不改，除抽取端账本）**：

- worktree 干净：`workspace` / `BA` / `code` / `Deploy` 四个，无遗留 feature/hotfix —— 与你 09:40 的结论一致。
- `demand` 与 `origin/demand` 同点（`92528ba`），无未推送提交。
- 账本漂移（抽取端，我已就地改）：**REQ-108** 登记表写"进行中"，实际 10-21 已合并 `a8a51ed`；**REQ-149** 写"进行中"，实际已合并 `9dfb8e7`（QA 报告 `demands/REQ-149/qa-review.md`，PASS 带非阻断）。
- 服务端账本漂移（**留给你**，我没动）：`demands/REQ-152/status.md` 仍写"进行中"，而登记表写"已完成 v0.2.0"（合并 `1137ea8`）。要么改 status.md，要么在 status 里写清"以登记表为准"。
- workspace 分支：**2026-10-27 09:45 已由 join 按用户裁定删掉 `.agents/`**（26 → 10 个被跟踪文件）。
  ⚠️ 但这与 `cleanup-log.md` 第 7 行（2026-09-28，**决策人＝项目负责人**）「`.agents/` + `AGENTS.md` 是已批准例外」**冲突**；
  同表需补记「已被 2026-10-27 裁定取代」，否则下次巡检会当未决纠纷重提（已发消息给 join，见 `inbox/join/20261027-0949-lion-02.md`）。
- 剩余 10 个文件中 `skills/**`（7 个）**lion 需要**（`ontok-build` / `ontok-extract` 是抽取规程，`ontok-query` 复核要读）——
  迁移前需先告知。

**我接下来要做的**：**暂不开工** —— 用户 2026-10-27 09:50 拍板「先不动工，只维持账本与复核」。
REQ-153 待用户给开发优先级后再建 worktree/派单。

---

## 2026-10-27 09:49 · 分工已确认（双向）

| 议题 | 结论 |
| --- | --- |
| AM（access mapping） | **生成归 lion**；`access validate` / `access fetch` / `access publish` 归 join |
| `coverage`（REQ-153） | 归 lion；只读、不校验不发布。**若需复用 `access` 解析器，先问 join，不自抄第二份 schema** |
| `relation verbs check` | 归 join；lion 出提案前会先跑它（`exit 4` = 不许冻结） |
| `version freeze` / `ontology export` | 归 join；lion ⛔ 不做冻结动作、⛔ 不碰 `ONTOK_VERSION` |
| 冻结前告知 | lion 交提案前给 join 一条消息 |

## 2026-10-27 09:49 · 账本审计（lion 只读复核）

报告 [`BA/docs/reports/2026-10-27-账本一致性审计.md`](../docs/reports/2026-10-27-账本一致性审计.md) ·
脚本 [`evidence/2026-10-27-ledger_audit.py`](../docs/reports/evidence/2026-10-27-ledger_audit.py)

| 检查 | 结果 |
| --- | --- |
| 同名重复（REQ-020/028/029 那类） | **0** |
| 已合并、登记表仍写未完成 | **0**（REQ-108 / REQ-149 收口后归零） |
| 登记表 × `status.md` 不一致 | **5**，全在服务端：REQ-140 / 141 / 151 / 152 / 155（已交 join，lion 不动他的文件） |
| 口径限制 | 「提交验证」只对 REQ-107 之后有效（早期为脱敏快照，`--grep` 零命中）；54/105 目录无 `status.md`，状态只有登记表一个来源 |

## 待 join 回复（低优先，不阻塞）

| # | 问题 | 为什么问 |
| --- | --- | --- |
| 1 | `ontok serve --access-mapping` 读的 schema 版本是 v1 还是 v2？校验入口在哪个文件？ | REQ-153 要读 mapping 判"有类无契约"，不想解析两套形状 |
| 2 | 服务面有没有"按类拿 catalog 列表"的**进程内**函数可复用（而非 HTTP）？ | 构建面不想为拿类清单起一个 serve |
| 3 | REQ-155 P3 换皮会不会连带改 `/api/catalog` 形状？ | 避免 coverage 按旧形状写类清单 |
