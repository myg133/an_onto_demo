# ADR-012 · 产品 skill 的唯一维护处 = `Deploy/skills/`

- 日期：2026-10-27
- 状态：**已接受**（用户当日口径：「skill 统一进 Deploy 里维护」）
- 关联：REQ-160（skill 里新增审核出口）、REQ-159（构建包）

---

## 一 · 决策

`ontok-build` / `ontok-extract` / `ontok-query` / `mcp.example.json` **只维护在 `Deploy/skills/`**（`deploy` 分支）。
仓库根 `skills/` **不再保留副本**（workspace 分支也不再跟踪）。

---

## 二 · 背景：这是对 2026-10-26 决定的反转

| 日期 | 决定 | 理由 |
| --- | --- | --- |
| 2026-10-26 | 从 `Deploy/` 迁到**仓库根 `skills/`** | 当时担心「两份副本漂移」 |
| **2026-10-27** | **搬回 `Deploy/skills/`** | 用户口径：skill 属于**交付物**，跟部署配置一起走；FDE 拿到 `Deploy/` 就有说明书 |

**为什么这次不算自相矛盾**：防漂移的关键**不是「放在哪」，而是「只有一份」**。
2026-10-26 的问题不是「skill 在 Deploy」，而是「Deploy 和根目录**同时**有一份」。
这次搬回时**根目录那份已删除**（`workspace 8881728` 删了 7 个文件），所以不存在两份。

---

## 三 · 后果（都已落地）

| 项 | 结果 |
| --- | --- |
| `Deploy/skills/` | 7 个文件（`README.md` / `mcp.example.json` / `ontok-build` / `ontok-extract` / `ontok-query` 的 `SKILL.md` + `AUTH-SPEC.md` + `RECIPE.md`）—— 提交 `Deploy 466d7f0` |
| `Deploy/README.md` | 表格补 `skills/` 行；把原来「skill 不在本分支」那句**反向改写**并记历史 |
| `Deploy/digital-employee/AGENTS.md` | 两处引用改**相对路径** `../skills/ontok-query/SKILL.md`（原来写 `skills/…`，那是相对**仓库根**的，搬走后会 404） |
| workspace 分支 | 只跟踪 **3 个文件**（`README.md` / `.gitignore` / `AGENTS.md`）—— `8881728`；`.gitignore` 去掉 `!/skills/` 白名单 |
| 根 `README.md` | 「内容都在哪」表加一行指向 `Deploy/skills/`（注明唯一维护处） |
| 引用约定 | 分支内互相引用用**相对路径**（`../skills/…`）；BA 文档引用写成 `Deploy/skills/…` |

---

## 四 · 副作用（正面）

workspace 分支从 **10 个被跟踪文件降到 3 个**，`cleanup-log.md` 里挂了很久的「workspace 跟踪 10 个文件 vs 规范 2 个」这条**基本收口**：
只剩 `AGENTS.md`（2026-09-28 项目负责人已批准的例外）。

---

## 五 · 以后怎么改 skill

1. 在 `Deploy/` worktree 里改 `skills/**`，提交到 `deploy` 分支；
2. ⛔ 不要在根目录或 `BA/` 里复制一份「方便看」——**副本一出现，漂移就开始了**；
3. 改完在 `BA/.local/ba-bus/` 给另一位 BA 一条消息（他会读 `skills/`）。
