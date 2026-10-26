# 状态：REQ-157 · 关系动词治理

## 当前状态

**P1+P2 已完成并上线（v0.2.9）· P3 提案就绪待签**

| 期 | 内容 | 状态 |
| --- | --- | --- |
| P1 | 字典 + `relation verbs check` + `semantic_cn` 从字典读 + altLabel 第五档 | ✅ `b6bea42` → `87385e7` |
| P2 | `parse.rs` 删枚举 + skill 补动词字典（两份） | ✅ |
| P3 | 重抽 v4 → **提案就绪，未签** | 🟡 见 `proposal-v4-naming.{json,md}` |

## 验收实测

| # | 判据 | 结果 |
| --- | --- | --- |
| AC-001 | 坏字典拒绝加载并点名 | ✅ 5 种（重复别名/别名撞主键/缺 version/缺 verbs/主键重复） |
| AC-002 | `uses` ⇒ forbidden + exit 非 0 | ✅ exit **4** |
| AC-003 | `attaches_to` ⇒ 归一 + exit 0 | ✅ `canonical=attached_to` |
| AC-004 | `xyz_unknown` ⇒ unknown + 3 候选 + exit 非 0 | ✅ 候选 `attached_to/based_on/depends_on` |
| AC-005 | 15 词标签不变 + REQ-156 守卫仍绿 | ✅ 5/5；变异（字典改「所属」）⇒ 两处同时红 |
| AC-006 | `parse.rs` 无旧枚举 | ✅ |
| AC-010 | altLabel 英文 ⇒ 发布被拒 | ✅ 真闸；**但加了「机器动词语形门」**（见下） |
| AC-011 | `semantic` 仍英文机器键 | ✅ |

**真实 v3 的 check 报告**（我复跑确认）：`exit=4` · `forbidden 2`（`uses` `associated_with`）· `deprecated 1`（`contains`）· `alias_normalized 2`（`attaches_to` `binds`）· `unknown 0` · 干净 29。

**P3 提案用 check 验过**：`--snapshot` ⇒ **exit 0 · 34/34 干净 · unknown 0 · forbidden 0**。

## 裁定：接受 altLabel 的「形状门」收窄

Dev 原样落地 AC-010 会把真实 v3 **判死**（`export` 由 exit 0 变 2，点名 `NAT`）——因为 altLabel 有两个来源：关系 meaning + **实体别名**。

改成「只抓全小写 ASCII/数字/下划线」的机器动词形态。**我用真实数据验了误杀面**：

| 项 | 数 |
| --- | ---: |
| v3 的 altLabel 总数 | 109 |
| 全小写形态（会被抓） | **0** |
| ASCII 但驼峰/大写（会放过，如 `NAT` `CLB` `AccountInfo`） | 42 |
| 含中文 | 67 |

⇒ **零误杀**，且 `uses`/`belongs_to`/`aggregates` 照抓。**接受。**

残留风险（记债）：将来若有实体别名写成全小写英文（如 `vpc`），会被判成机器动词 ⇒ **导出失败（fail-closed）**，方向安全但会误伤。

## 附带修的一个镜像构建 bug

v0.2.8 **镜像构建失败**：`Dockerfile` 漏 `COPY relation-verbs.json`，而 `include_str!("../../relation-verbs.json")` 在 **本地 `cargo build` 是绿的**。

- 已补 COPY
- 新增守卫 `test_dockerfile_covers_include_str`（src 里每个 `../../X` 必须在 Dockerfile 有 `COPY X`），变异自证红→绿
- **v0.2.8 作废**（tag 存在但无产物），改发 **v0.2.9**
