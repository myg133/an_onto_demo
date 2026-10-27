# 状态：REQ-160 · 构建端两种模式 + 抽取后自动起审核台

## 当前状态

**进行中**（2026-10-27 · 已派 Dev，worktree `feature-REQ-160`，基线 develop `e7c29aa`）

## 一句话

> 两条路进来（**独立 agent 读 skill 自己抽** / **人敲命令驱动 ACP 抽**），**一条路出去**：
> 抽完就起一个本地 console，让人在页面上审核、确认、签字。

## 两种模式

| | agent 模式 | acp 模式 |
| --- | --- | --- |
| 驱动者 | 独立 agent（只读 skill 包） | 人在终端敲命令 |
| 我们的交付 | **skill 包** + 命令 | 命令编排 |
| 收口 | skill 末步指示 agent 跑 `review open` | 编排命令自己跑 `review open` |
| 共同 | **都不自动 apply / freeze**；人签只在 console 页面上发生 | 同左 |

## 收口命令（本需求的核心）

```bash
ontok-builder review open --project <p> [--version <v>] [--bind 127.0.0.1:0] [--no-browser]
```

- 内存一次性 admin token（**不落盘、不进 URL**），同进程复用 `console::serve::serve`
- 端口冲突自动顺延；只绑回环；Ctrl-C 退出即失效
- 进度：三步 —— 校验项目 → 起服务 → 打印 URL + 口令 + 开浏览器

## 还有一条编排（acp 模式）

```bash
ontok-builder build run --project <p> --input <docs> --agent acp --agent-command pi-acp --agent-skill <path>
```

`doc ingest → doc merge → proposal import → review open` 四段串成一条；任一阶段失败即整体失败、零产物。

## 验收（8 条，见 demand.md §七）

AC-001 起得来要口令 · AC-002 URL 无口令 · AC-003 端口顺延 · AC-004 退出即失效 ·
AC-005 只绑回环 · AC-006 acp 一条命令到审核台 · AC-007 不自动签 · AC-008 skill 与二进制同版本

## 待办

- [x] **skill 部分**（用户 11:20 直接点出「skill 里需要一个审核的流程或操作项」）—— 已做，见下
- [x] **等 REQ-159 落地再派**（已落地：`e7c29aa`）
- [x] 派 Dev：`review open` + `build run` —— 已交付 4 个 commit（`183003e` / `c24fd2d` / `2067d6d` / `4cb83dd`）
- [ ] **Q4 判据修正**（`no_zip_crate_was_added` 与 `flate2` 冲突）—— 已派回 Dev
- [ ] QA：重点 AC-002（URL 无口令）/ AC-004（不落盘）/ AC-007（不自动签）

## 交付与我的复核（2026-10-27 12:0x）

| 命令 | 结果 |
| --- | --- |
| `cargo check --lib` | 绿 |
| `cargo test --lib review_open` | **20 passed / 0 failed**（我复跑） |
| `cargo test --test test_req160_review_open` | **7 passed / 0 failed**（我复跑） |
| `cargo test --test test_req159_bundle` | **32/33** —— 唯一红是 `no_zip_crate_was_added`，见 Q4 |

## 六个问题的裁定（BA）

| # | 问题 | 裁定 |
| --- | --- | --- |
| **Q4** | `no_zip_crate_was_added` 与 `Cargo.toml` 的 `flate2 = "1"` 冲突 | **允许 `flate2`，⛔ 禁止 `zip` 库**。理由：`flate2` 早就在依赖树里（`ureq` 的 gzip feature 带进来，Cargo.toml 已登记同版本同 feature）；服务端读第三方 `zip -9` 的包**需要 deflate**。「不新增 crate」的本意是不往依赖树加东西，不是不许出现二进制解析。改判据（改名 + 只查 `zip` + `flate2` 必须有「依赖树」注释），⛔ 不删依赖 |
| **Q1** | `review open` 是否必须 `ONTOK_SECRET_KEY` | **v1 保持要求**（`ConsoleService` 无条件持有 DSN 加密器；为审核降级会把启动 fail-fast 变成运行时 500）。把这条写进 `--help`；「没密钥也能审」记后续单 |
| **Q2** | 最新版已冻结时 | **保持报错**，⛔ 不自动 `version new` —— 悄悄造版本比报错危险 |
| **Q3** | 口令要人手动粘进页面 | **接受**。这是 S2 的直接后果：口令只进终端一行 + 页面 sessionStorage |
| **Q5** | AC-006「版本都产出」 | **我写错了，已订正**：`proposal import` 不产版本（`apply` 才产）；正确终点是「断言 + 提案已导入 + 项目/基版本就绪，**未 apply 未 freeze**」 |
| **Q6** | 审核台算不算「出口」 | **不算**：只绑回环、只打印到本机终端、不产生客户可见数据 ⇒ 不进 t5 出口登记；要求在模块头写明理由 |

## 未验（⛔ 不许当通过）

| 项 | 原因 |
| --- | --- |
| 完整 `review open` 真起服务 + 浏览器里看到待审清单 | 本机无元数据库 |
| `build run` 成功路径（含 import 落库） | 同上 |
| AC-007 的「版本真的未冻结」（运行时） | 同上；目前只有结构判据（源码无 apply/freeze） |

## skill 部分已完成（2026-10-27）

| 文件 | 改了什么 |
| --- | --- |
| `skills/ontok-build/SKILL.md` | 新增 **§三·六 审核台：人签在这一步发生**（起法 `review open` / 退回 `console serve`、页面四步与判据、**四条禁令**、签完才 `bundle pack`）；把路径 A/B 的 `--accept-all` 从主流程降为「人明确全部接受时」；检查单加 4 项；头加「适用工具版本」 |
| `skills/ontok-extract/SKILL.md` | 新增 **§十 抽完之后：交给人审核**（你的任务到此为止、交接口径、**四条不许代签**） |

**顺带修了一处会直接坑人的陈旧**：两份 skill 里的构建命令还写着 `ontok doc ingest` / `ontok ontology export`，
而 REQ-152 之后它们在 `ontok-builder` ⇒ **21 处命令已改**。

> ⚠️ **更正（join 2026-10-27 11:17 实证，我原先写错）**：`console` **也是构建面** ——
> `ontok console serve` 不存在，正确写法是 **`ontok-builder console serve`**。
> `ontok`（服务面）的子命令只有 **`query` / `mcp-server` / `serve`** 三个。
> skill 里那两处由 join 在 `Deploy 38f50e0` 修正；本条 status 原先写的「`console` 仍是服务面」是错的。
>
> **根因**：`console` 名字像控制面，实际在 REQ-152 拆二进制时和构建面一起搬走了 ——
> 判断「哪个二进制」要按**清单**，不能按名字像什么。

⇒ AC-008（skill 与二进制同版本 / 无「自动接受」指令）**现在可判**：两份 skill 里 `--accept-all` 只出现在「⛔ 不许」的上下文。


## 真库 + 真服务运行期验证（2026-10-27，lion 亲跑）

环境：podman 容器 `ontok-pg`（postgres:16，`127.0.0.1:5432`）+ 我专用库 `ontok_lion_dev`。
详细报告：[`docs/reports/2026-10-27-真库真服务端到端验证.md`](../../docs/reports/2026-10-27-真库真服务端到端验证.md)

| AC | 之前 | 现在 |
| --- | --- | --- |
| AC-001 起得来、要口令 | 结构判据 | ✅ **真起服务**：`GET /` 匿名、`/api/projects` 无口令 401 / 陌生口令 401 / 一次性口令 200 |
| AC-002 URL 无口令 | 单测 | ✅ **真 URL**：`http://127.0.0.1:47067/`，页面地址栏与正文均无 token |
| AC-003 端口顺延 | 单测（占端口） | ✅ 内核分配端口 + 打印真实端口（47067） |
| AC-004 退出即失效 / 不落盘 | 结构判据 | ⚠️ **部分**：进程已 kill、端口不再监听；「全盘搜口令」尚未在 kill 后重跑一次（下一轮补） |
| AC-005 只绑回环 | 真进程 | ✅ 同上 |
| AC-006 build run 一条命令 | 未验 | ⬜ 仍缺（要 agent 子进程；下一步补） |
| AC-007 不自动签 | 结构判据 | ⬜ 运行时仍未验 |

**Playwright 打真服务 11/11**（不 stub 任何 API），含「评审页载入到真清单」「构建包 tab 显示真项目」。

## 真库跑出来的两个发现（不属本需求，但要有人接）

| # | 发现 | 影响 |
| --- | --- | --- |
| **F-1** | `bundle pack --project anspire_real` **失败**：项目名含下划线不符合 manifest schema 的 pattern | **历史项目都打不了包**（`anspire_real` 是真实存在的项目名）⇒ 要么放宽 schema 允许 `_`，要么给项目改名路径 |
| F-2 | 快照里有业务列叫 `password` ⇒ `bundle.credential.suspect` 稳定误报 | 告警疲劳；建议判据收紧成「带值的 password 键」 |
