# QA-B · 严重度判定 与 处置建议

## 4 · 严重度（逐条回答）

### Q1. 这算不算「REQ-019 想消灭的静默假溯源」？

**算，而且是最严重的那一档 —— 但要精确区分「同一类问题」与「同一处代码」。**

- 同一类：**是**。REQ-019 消灭的正是
  「`extracted_by` 记的 agent 与 items 的真实来源不一致，且无声」
  （`orchestrate.rs:290-294` 原话：「假溯源不是『差一点对』，是**全错**…而且没人会发现」）。
- 同一处代码：**不是**。REQ-019 的闸门 `require_proposal_producing`
  装在 `orchestrate()` 入口（`orchestrate.rs:356-361`），**只管 orchestrate 这条路**。
  `propose import` 走 `cli.rs:417-466`，**完全不经过 orchestrate**。

⇒ REQ-019 **没有漏检**，它守的门是对的。
问题在于**盘子里本来就有四条路，REQ-019 只封了其中一条**。

### Q2. 绕过能力闸门，还是压根在闸门之外？

**答案：压根在闸门之外 —— 另一条独立的路。**（这两者性质不同，BA 需要分开记账）

证据 —— 盘子里实际存在的写路径：

| # | 路径 | 入口 | 过 REQ-019 闸门？ | extracted_by 来源 |
| --- | --- | --- | --- | --- |
| 1 | `orchestrate` 编排 | `agent/orchestrate.rs:356` | ✅ 过（`:361`） | `agent.identity()`，闸门保证名实相符 |
| 2 | `propose::extract` 公开入口 | `extract/propose.rs` | ❌ 不经过 | **已被 REQ-020 收窄为 `pub(crate)`**（`:261-271`） |
| 3 | **`ontok propose import`** | `cli.rs:417-466` | ❌ 不经过 | **JSON 文件里的任意值** |
| 4 | `PgProposalStore::create` 直接调用 | `db/proposal.rs:108` | ❌ 不经过 | 任意 `Option<ExtractedBy>` |

- 路径 2：REQ-020 已用**编译期**手段封死（`pub(crate)`）—— 这是**干净**的封法，
  `propose.rs:268-271` 的注释说得很清楚：「crate 外拿不到它，也就没有
  『不经过 orchestrate 的闸门却产出 Proposal』这条路」。
- 路径 3：**Dev 报的问题，公开 CLI，编译期封不住**（`cli` 是 `pub mod`，见 `lib.rs:10`）。
- 路径 4：`lib.rs:9` `pub mod db` ⇒ crate 外任意 Rust 消费方可直接调。

⇒ **性质判定：不是「绕过」，是「压根不在闸门的守卫范围内」。**
Dev 把它说成「绕过溯源守卫」，措辞上不够准确（这会影响 BA 的记账与优先级判断）。

### Q3. 谁能触发？需要什么？

| 要素 | 事实 | 依据 |
| --- | --- | --- |
| 本地 shell | **需要** | 是本地 CLI，跑在有 shell 的机器上 |
| DB 写权限 | **需要**（经 `ONTOK_DATABASE_URL`） | 但凡能 import 提案的人本来就有 |
| 特殊知识 | **几乎不需要** | 照抄 `fixtures/proposal-demo.json` 改两个字符串即可；`--help` 一眼看会 |
| 网络 / 外部配合 | **不需要** | 全程本地 |
| 提权 | **不需要** | 普通用户权限即可 |
| 双轨审核 | **需有人 `accept-all`** | 但这是**正常流程的一部分**，不构成障碍 |

**关键**：攻击者**不需要**绕过任何东西 —— 他走的是一条**被文档化、被 fixture 示范、
被测试依赖**的正常路径。唯一"越界"的只是 `extracted_by` 字段的内容。

**信任边界判定**：能跑 `ontok` + 能写库的人，**本来就能**通过
`review --accept-all` + `proposal apply` 往库里写任意本体内容。
所以这**不是权限提升**，不是安全漏洞，是**数据可信度问题**。
把它当安全漏洞报会拉高整件事的处置等级，也会让 ADR 讨论跑偏。

### Q4. 对 §8.4「agent 升级后解释 diff 来源」的实际危害？

**危害真实但有前置条件，且是「可检测的错」而非「不可检测的灾」。**

危害链条：
1. 有人手写 JSON 或脚本批量生成提案（这在 REQ-020 之后会成为常态 ——
   RN-2 的 `CandidateType` 契约改造会产出更多程序化提案）；
2. `extracted_by.agent/version` 写错或随手复制；
3. 某天 agent 真的升级了，diff 出现；
4. 查 `extracted_by` → 看到的是错的版本号 → **归因结论错**，
   可能去「验证」一个根本没变过的 agent 版本，或漏掉一个真的变了但没记的。

**为什么不是灾难**：
- `extracted_by` **不参与收敛判定**（`snapshot.rs:37` 注释：`failed_groups`
  「不参与收敛判定」；`ExtractedBy::fully_succeeded()` 只看 `failed_groups` 非空）。
  伪造 `extracted_by` **不会**让系统错误地认为「收敛了」——
  收敛还受 §8.5 终止条件与冻结控制。
- v1/v2 的父子关系由 `seq` + 父版本承担（ADR-001 明写「演进链不由 `extracted_by` 承担」），
  **伪造 `extracted_by` 不会**破坏版本链本身。
- 发现手段存在：diff 看着不合理时，人可以回头看 `proposal_items` 的 `evidence`
  （`payload.evidence`，`fixtures/proposal-demo.json:24` 有真实来源表列）。
  **只要有人去核对 evidence，假溯源是暴露的。**

**但有一个真实的静默面**：`failed_groups`。
`orchestrate.rs:308-309` 指出，REQ-019 要挡的正是
「agent 整组失败（failed_groups 非空）时 items 照样是满的，
『什么都没抽』与『抽得很全』在产物里长得一样」。
我构造的 JSON 里 `failed_groups: ["payments","risk"]` 同样是我编的，
系统照单全收（实测已证）⇒ **抽取失败这件事可以被人为隐藏或伪造**，
而 `fully_succeeded()` 会据此给出**错误的成功/失败判断**。
这一条比 `agent`/`version` 写错更值得警惕。

### 严重度结论

| 维度 | 评级 | 理由 |
| --- | --- | --- |
| 数据完整性 | **中** | 可写入错误的溯源元数据，污染 §8.4 的归因依据 |
| 安全性 | **低（非安全漏洞）** | 需要已有写权限，不构成权限提升 |
| 静默性 | **高** | 无警告、无校验、无留痕，失败伪装成成功（`failed_groups` 尤甚） |
| 可检测性 | **中** | diff 异常时可回溯 `evidence` 发现，但需要人主动去查 |
| 是否 REQ-020 漏做 | **否** | REQ-020 守的是路径 2，路径 3/4 不在其 scope（见 `.feature/manifest.json` 仍写 REQ-002） |

**综合：中severity 的架构缺口 / 数据可信度问题，不阻塞 REQ-020，但必须显式记账。**
