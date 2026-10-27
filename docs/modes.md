# 两种运行模式：谁加载什么 · 谁起服务 · 谁签字

> 两种模式下**取数都一样**：API 源用 [`ontok-query/scripts/access_fetch.py`](ontok-query/scripts/access_fetch.py)，⛔ 别按 AUTH-SPEC 自己实现 TC3 —— 跑法见 [`ontok-query/SKILL.md`](ontok-query/SKILL.md) §4.5。
> 一页说清 **ACP 模式** 与 **独立 agent 模式** 的差别。
> 先记住三句共同点：
>
> 1. **抽取契约只有一份**（`ontok-extract`）—— 两种模式共用，⛔ 不按模式拆两份。
> 2. FDE 的流程手册是 [`../docs/fde-guide.md`](../docs/fde-guide.md)（**不在 skills/ 里**），两种模式下都是**人**读。
> 3. **签字 / 定版 / 传 Studio 永远是人**；agent 的终点是**草稿**。

---

## 一 · ACP 模式（工具主动驱动外部 agent）

```
FDE                     ontok-builder                       外部 agent（如 pi-acp）
 │                            │                                      │
 │  build run --agent acp ───▶│                                      │
 │    --agent-command <cli>   │                                      │
 │    --agent-skill <ontok-extract/SKILL.md 的路径>                    │
 │                            ├─ ① 把 skill 路径交给对端 ────────────▶│ 读 SKILL.md
 │                            │                                      │ 按契约抽：
 │                            │                                      │ 概念/关系/逐字证据
 │                            ├─ ② doc ingest ◀──────────────────────┤ 断言 JSONL
 │                            ├─ ③ doc merge                        │
 │                            ├─ ④ 写草稿 out/proposal.json          │
 │                            │                                      │
 │                            └─ ✅ 四段跑完**自动**起评审台 ─────────▶ 本机 console（无口令）
 │  浏览器：逐条两轨 ✓✗ → 定版 → 导出构建包                              │
 │       （FDE 本人操作；agent 到 ④ 就结束，⛔ 不签不定版）              │
 │  人工把 .ontokbundle 传到 Studio                                     │
```

**要点**

| # | 要点 |
| --- | --- |
| 1 | skill 由 **`--agent-skill`** 传**路径**给对端，FDE 不用手工往别人的 skills 目录里装 |
| 2 | 四段由工具**串行编排**：任一段失败 ⇒ 整体失败、**零产物**（不产出半份） |
| 3 | `build run` **只到草稿**：⛔ 不 apply、不定版 |
| 4 | 评审台由**工具自动**起；`--no-browser` 只影响开不开浏览器，**不影响起不起来** |

---

## 二 · 独立 agent 模式（用户自己开的 agent）

```
FDE / 用户            自己的 agent（Claude Code / Codex / …）        ontok-builder
 │  把 ontok-extract/SKILL.md 装进该 agent 的 skills 目录（或直接贴进上下文）
 ├─────────────────────▶│ 读契约 → 抽取 → 断言                     │
 │                      ├──────────────────────────────────────────▶│（用户把断言并成草稿）
 │  草稿放到 <工作目录>/out/proposal.json                             │
 ├─ console serve --dir <工作目录> ──────────────────────────────────▶ 本机评审台（无口令）
 │  浏览器：逐条签 → 定版 → 导出构建包                                 │
 │  人工把 .ontokbundle 传到 Studio                                    │
```

**要点**

| # | 要点 |
| --- | --- |
| 1 | skill 由**人**分发（目录 / 贴上下文），工具不参与 |
| 2 | 阶段顺序由**人（或人的 agent）**保证：⛔ 别跳「合并成草稿」直接签 |
| 3 | 评审台要**人手动**起：`console serve --dir <工作目录>` |
| 4 | 草稿路径是**约定**：`<工作目录>/out/proposal.json`（评审台只读这一个路径） |

---

## 三 · 差在哪、不差在哪

| 维度 | ACP 模式 | 独立 agent 模式 |
| --- | --- | --- |
| 谁驱动 agent | `ontok-builder build run` 主动驱动 | 用户自己开、自己发指令 |
| skill 怎么进 agent | 工具用 `--agent-skill` 传路径 | 人装进 skills 目录 / 贴上下文 |
| 谁保证四段顺序 | 工具编排（失败即整体失败、零产物） | 人 / 人的 agent |
| 谁起评审台 | **工具自动** | **人手动** `console serve --dir` |
| **不差的部分** | 抽取契约（同一份 `ontok-extract`）· 草稿形状（`out/proposal.json`）· 评审动作（逐条两轨签）· 导出物（只含**定版**的 `.ontokbundle`） | 同左 |

---

## 四 · 谁负责什么（最后一列才是「谁」）

| 动作 | ACP 模式 | 独立 agent 模式 | 负责方 |
| --- | --- | --- | --- |
| 写/维护抽取契约 | 工具把它传给对端 | 人装进 agent | **产品**（只一份） |
| 抽取：概念 / 关系 / 逐字证据 | 外部 agent | 用户的 agent | **agent** |
| 合并成草稿 | `build run` 第 ③ 段 | 人跑 `doc merge` | **agent / 工具** |
| 逐条两轨签（业务 / 建模） | console v2 页面 | console v2 页面 | **人（FDE）** |
| 定版（冻结快照） | 页面「定版」按钮 | 同左 | **人** |
| 导出构建包 | 页面「导出构建包」 | 同左 | 人（点一下） |
| 传到 Studio | 人工 | 人工 | **人** |

> ⛔ 把「签 / 定版 / 上传」交给 agent，等于把「有人负责」变成「没人负责」（ADR-005）。

---

## 五 · 两条模式的产物是同一个形状

```
<工作目录>/out/proposal.json                         草稿（agent 产出，只读）
<工作目录>/.ontok/decisions.json                     人的裁决（唯一可写状态）
<工作目录>/.ontok/versions/v<N>/snapshot.json        定版（冻结，不可改）
<工作目录>/.ontok/versions/v<N>/meta.json            {seq, created_at, items, 两个 sha256}
<工作目录>/.ontok/bundles/<目录名>-v<N>.ontokbundle  导出包（只含定版）
```

✅ 所以**下游不用问"这是哪个模式做的"**：看文件就知道走到哪一步了。

---

## 六 · 常见误解

| # | 误解 | 事实 |
| --- | --- | --- |
| 1 | 「ACP 和独立 agent 各要一份契约」 | ❌ 一份。差别只在**谁分发** |
| 2 | 「独立模式下 FDE 指南也要给 agent」 | ❌ 它是 FDE 的手册（在 `docs/`），给 agent 只会让它越界 |
| 3 | 「ACP 模式抽完就自动生效」 | ❌ 自动的只是**起评审台**；签字/定版仍要人 |
| 4 | 「导出可以随时导」 | ❌ **未定版不能导出**；导出的是**定版**，不是草稿 |
| 5 | 「console 传 Studio」 | ❌ console 不上传、不入库、不激活；**人工**把包传给 Studio |
