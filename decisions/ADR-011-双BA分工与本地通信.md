# ADR-011 · 双 BA 同机协作：分工按子命令划，通信走本地文件、不走 git

- 日期：2026-10-27
- 状态：**已接受**（用户当日口径：两个 BA 共用一个工作目录；通信在本地文件里进行）
- 关联：REQ-153（归属）；REQ-152（执行器移出产品，决定了分工线）

---

## 一 · 决策

1. **同机双 BA 共用同一份工作目录与同一个 `.git`**（`join`=服务端、`lion`=本地抽取端）。
   不存在"各自 clone、靠 pull 合并"这回事 —— 双方写的是同一棵树。
2. **分工按 `ontok-builder` 的子命令划**，不按二进制划（`ontok-builder` 是同一个 bin，
   光说"builder 归 lion"会与服务面的 `access` 打架）。
3. **日常通信走本地文件信箱** `<repo-root>/.local/ba-bus/`（不跟踪）：
   `board/<我>.md`、`presence/<我>.txt`、`inbox/<对方>/`。
4. **git 只承载"结论"**：本 ADR、需求文档、登记表、审计报告。
   ⛔ 不把 commit 当聊天记录（一条协调信息 = 一条 commit 会污染历史、且 push 是共享动作）。

---

## 二 · 为什么通信不放 git

| 备选 | 为什么不用 |
| --- | --- |
| 每次协调都 commit | 单文件 100 多行 commit message 已经出现过（如 REQ-149 那行），再叠加对话式提交会让 `git log` 不可读 |
| 远程/消息队列 | 同机、同一棵树，没必要，且多一个故障点 |
| `BA/.local/` | 挂在 `demand` worktree 上，worktree 一收就没了 |

**代价（必须承认）**：`.local/` 不跟踪 ⇒ 信箱内容**不随仓库走**，机器一换就没了。
⇒ 因此**结论必须落到跟踪文件**（本 ADR / 登记表 / `status.md`），信箱只放过程。

---

## 三 · 分工（子命令级）

| 子命令 / 产物 | 归属 | 依据 |
| --- | --- | --- |
| `doc ingest` / `propose` / `import` / `review` / `proposal` / `concept` / `name` / `exclude` | **lion** | 抽取链路 |
| 本体构建：提案 → 审核 → 落库（`apply`） | **lion** | 冻结之前的一切 |
| AM（access mapping）**生成** | **lion** | 内容来自抽取结果 |
| `coverage`（REQ-153） | **lion** | 只读、不校验不发布 |
| `access validate` / `access fetch` / `access publish` | **join** | 契约与服务面 |
| `relation verbs check` | **join** | 字典是冻结判据（lion 出提案前会先跑，`exit 4` = 不许冻结） |
| `version freeze` / `ontology export` / `ONTOK_VERSION` | **join** | 发布与客户视图 |
| `ontok` 服务面（query serve / Studio / REST+MCP / 契约）、`Deploy/` | **join** | —— |

**握手点**（跨面改动前先发消息，不等回复也可以先做，但必须告知）：

1. **AM 契约形状**：`access validate` 与 `serve --access-mapping` 认同一份 schema。
2. **catalog / describe 字段**：抽取端填什么，客户就看到什么。
3. **版本号**：lion 交提案，冻结与线上切换归 join。

---

## 四 · 2026-10-27 的现状（写下来免得下个会话重查）

| 项 | 事实 |
| --- | --- |
| 账本 | 152 行登记表 × 105 个需求目录审计完毕：同名重复 0、已合并仍写未完成 0、登记表×`status.md` 不一致 5（REQ-140/141/151/152/155，**全在服务端**，已交 join） |
| 审计报告 | [`docs/reports/2026-10-27-账本一致性审计.md`](../docs/reports/2026-10-27-账本一致性审计.md) + 可复跑脚本 `evidence/2026-10-27-ledger_audit.py` |
| 抽取端待办 | REQ-153（已就绪，依赖满足）、REQ-148（未排期）、REQ-146（未排期）；**用户当日要求"先不动工，只维持账本与复核"** |
| push 策略 | lion 每次 push 前问用户；join 的 push 会顺带把 lion 未推的 commit 带上去（同分支同机，性质上是共享动作） |
| `.agents/` | join 按用户裁定删除，改由全局技能接管；`cleanup-log.md` 2026-09-28 那条旧例外仍写着"保留"，需补记"已被 2026-10-27 裁定取代" |

---

## 五 · 后果

- 新 BA 会话入场只需读三处：本 ADR（分工与结论）、`dispatch/agent-registry.md`（谁在线）、`.local/ba-bus/board/`（对方在干什么）。
- 任何"只在信箱里说过、没落跟踪文件"的约定，**视为没定**。
