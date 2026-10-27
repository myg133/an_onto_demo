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

---

## 六 · `skills/` 只放 **agent 加载的东西**（2026-10-27 补充，用户提出）

**事实起因**：`skills/ontok-build/SKILL.md`（28 KB）其实是 **FDE 读的流程手册**，却和有 `name`/`description` frontmatter 的 agent skill 放在同一目录 ——
用户在评审时指出：**给人看的文档不该放在 `skills/` 里，应该放 `docs/`，名字用 `fde-guide.md` 这类**。

**为什么这是对的**（不是命名洁癖）：

| 目录 | 语义 | 判据 |
| --- | --- | --- |
| `skills/` | **agent 加载** | 有 `name` + `description` frontmatter ⇒ agent 会拿它当可用技能，按 description 挑 |
| `docs/` | **人读** | 没有 frontmatter；人按文件名找 |

放错目录的真实代价：agent 看到 `ontok-build` 会以为"这是我可以执行的技能"，
而它的内容是"人怎么一步步做实施" ⇒ **要么越界执行，要么浪费上下文**。

**处置（`deploy` 分支 `59c8ebf` + `dd4850a`）**：

| 改动 | 结果 |
| --- | --- |
| `skills/ontok-build/SKILL.md` → **`docs/fde-guide.md`** | 去掉 frontmatter，改成「这份是给人读的」文档头 + 指向 `../skills/*` |
| `skills/MODES.md` → **`docs/modes.md`** | 同一条规矩（也是人读的模式说明） |
| `skills/` 现在只剩 | `ontok-extract/`（抽取 agent 契约）· `ontok-query/`（数字员工流程）· `mcp.example.json` · `README.md`（目录索引） |

**判据（以后新增文件时问一句）**：**这份东西是给 agent 加载的，还是给人读的？**
- agent 加载 ⇒ `skills/<名>/SKILL.md` + frontmatter
- 人读 ⇒ `docs/<名>.md`，⛔ 不要加 frontmatter（加了就会出现在 agent 的技能列表里）
