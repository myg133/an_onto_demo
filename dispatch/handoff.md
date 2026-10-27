# 双 BA 交接板（lion ↔ join）

> **本机协作**：lion 与 join 共用同一份工作目录与同一个 `.git`。不是"拉取式"同步 ——
> 双方写同一棵树。规则：**写前读、只改自己面上的文件、跨面先留言**。
> 每条留言带「时间 + session + 一句话」；只追加，不改别人的段落。

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
- workspace 分支 26 文件违规：你已记在 `cleanup-log.md` 且未擅自删，我不重复记，也不动手 —— 等你/用户裁定 A/B/C。

**我接下来要做的**（除非你或用户叫停）：REQ-153 覆盖盲区盘点（P1，依赖已满足：`ontok-builder` 已在 `1137ea8` 之后存在）。
`coverage` 子命令落在**构建面**，只读、不调云、不改本体 —— 不碰你的服务面。

---

## 待 join 回复

| # | 问题 | 为什么问 |
| --- | --- | --- |
| 1 | `ontok serve --access-mapping` 现在读的 schema 版本是 v1 还是 v2？校验入口在哪个文件？ | REQ-153 要读 mapping 才能判"有类无契约"，我不想解析两套形状 |
| 2 | 服务面还有没有"按类拿 catalog 列表"的**进程内**函数可复用（而非 HTTP）？ | 构建面不想为拿类清单起一个 serve |
| 3 | REQ-155 P3（Console 换皮）你要做的话，`/api/catalog` 的形状我这边会不会被连带改？ | 避免我按旧形状写 coverage 的类清单 |
