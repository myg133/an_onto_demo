# 需求：REQ-160 · 构建端两种模式 + 抽取完成后自动起审核台

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-160 |
| 优先级 | **P1**（决定 FDE 怎么用这个工具，不决定能力边界） |
| 状态 | **已就绪** |
| 基线 | `develop` `aa6e52d`（REQ-159 三线并行期间不影响本需求） |
| 触发 | 用户 2026-10-27 10:15：builder 要承载两种模式，**两者抽取完成后都要启动 console 让用户审核确认** |
| 关联 | REQ-124（console 控制面）、REQ-147（`doc ingest --agent acp`）、REQ-159（构建包上传） |

---

## 一 · 一句话

> 构建端有两条路进来（**独立 agent 读 skill 自己干** / **人敲命令驱动 ACP 干**），
> 但**出口只有一条**：抽完就起一个本地 console，让**人**在页面上审核、确认、签字。

---

## 二 · 两种模式（区别只在「谁在驱动」）

| | **agent 模式** | **acp 模式** |
| --- | --- | --- |
| 谁在跑 | **独立的 agent**（Claude / Codex / 客户现场的 agent），只读我们给的 **skill 包** | **人**在终端敲 `ontok-builder` 命令，ACP 子进程只负责「一块一块抽」 |
| 我们的交付物 | **skill 包**（`ontok-extract` 等）+ 命令 | 命令本身 |
| 中间产物 | 与 acp 模式**同一份 schema**（REQ-147 已保证） | 断言 JSONL → `doc merge` → 提案 |
| 收口 | skill 的最后一步指示 agent 执行 `review open` | 编排命令自己执行 `review open` |
| 共同点 | **都不自动 apply / freeze**；人签只在 console 页面上发生 | 同左 |

**为什么两条路必须同一个出口**：出口一分叉，就一定会出现「agent 模式跳过了人签」或
「acp 模式产物形状不同」两种事故。REQ-147 已经把**产物 schema** 收敛成一份，本需求把**收口动作**也收敛成一条命令。

---

## 三 · 统一收口：`ontok-builder review open`

```bash
ontok-builder review open \
  --project <p> \
  [--version <v>]          # 省略 = 该项目最新版本
  [--bind 127.0.0.1:0]     # 0 = 自动选空闲端口（默认）
  [--no-browser]           # 不给就不自动开浏览器（CI / 远程 SSH 用）
  [--token-ttl 4h]         # 一次性口令的有效期（默认 4 小时）
```

**行为（按顺序）**

| # | 步骤 | 说明 |
| --- | --- | --- |
| 1 | 校验项目 / 版本存在（**只读**，不迁移、不写） | 不存在 ⇒ 退出码 2 + 点名项目与版本 |
| 2 | **生成一次性 admin token**（≥ 32 字符，进程内存，**不落盘**） | 复用 `console::tokens::AdminTokens` 的内存构造，⛔ 不写临时文件 |
| 3 | 起 `console::serve::serve`（**同进程**，不是 spawn 子进程） | 复用既有服务，⛔ 不新写一套 HTTP |
| 4 | 端口冲突自动顺延（`--bind` 给 `:0` 时由内核分配） | 打印**实际** URL |
| 5 | 打印「URL + 一次性口令 + 有效期」，并默认调用系统浏览器打开 URL | 口令**只进本地终端**，⛔ 不进 URL、⛔ 不进日志 |
| 6 | 前台运行，Ctrl-C 退出；退出即 token 失效 | 无「后台常驻」形态 |

**安全边界（必须写进 `--help`）**

| # | 规则 |
| --- | --- |
| S1 | **只绑回环**。非回环地址必须显式 `--allow-public`（沿用 console 现有纪律，`src/console/serve.rs`） |
| S2 | 口令是**一次性**的：进程退出即失效；不写文件、不写 URL、不写 sessionStorage 以外的任何地方 |
| S3 | 页面壳的既有纪律全部不变：零数据、禁 `localStorage`、CSP 自包含（`src/console/page.rs` 头部） |

---

## 四 · acp 模式：一条命令跑完并起审核台

现状是四段手工（REQ-147 的实测路径）：

```bash
ontok-builder doc ingest --input docs/ --out a.jsonl --stats s.json --agent acp --agent-command pi-acp --agent-skill <skill>
ontok-builder doc merge  --assertions a.jsonl --project <p> ...
ontok-builder propose import --proposal <p1> ...      # 已有：proposal import
# 然后人才想起去起 console
```

**要做**：一个编排入口，把「抽取 → 合并 → 导入 → 起审核台」串起来：

```bash
ontok-builder build run \
  --project <p> \
  --input <docs-dir|file> \
  --agent acp --agent-command pi-acp --agent-skill <path> \
  [--strict-deterministic] \
  [--no-browser]
```

| 裁定 | 内容 |
| --- | --- |
| R1 | **任一阶段失败即整体失败**，退出码非 0，且**不留下半成品**（沿用 REQ-147 的 `--strict-deterministic` 语义：宁可一个产物都不写） |
| R2 | 末尾**自动**执行 `review open`；`--no-browser` 只影响开不开浏览器，**不影响起不起来** |
| R3 | ⛔ **不自动 apply / freeze**。`build run` 的终点是「人在页面上看到待审清单」，不是「本体已生效」 |
| R4 | 中间产物落在 `--work-dir`（默认 `.ontok-build/`），可重复跑、可断点续（缓存命中即 0 token） |
| R5 | 不做「一个命令支持所有源」：本需求只做**文档路径**（库路径 REQ-003 已有，是另一条链） |

---

## 五 · agent 模式：skill 包（**人分发，不做导出命令**）

用户原话：「我们将提供 builder 的使用 skill 包，独立 agent 通过独立 skill 包进行抽取」，
并在 11:50 明确：**skill 就在 `skills/` 目录里，由人来分发**（不另做 `skill export`）。

| # | 要做 | 理由 |
| --- | --- | --- |
| 1 | **`skills/ontok-extract/SKILL.md` 末尾补「收口」一节**：抽完执行 `ontok-builder review open --project <p>` | 否则 agent 抽完就停，人到不了审核台 |
| 2 | skill 头补一行「**适用工具版本**」，并写明版本不匹配时先升级工具 | 人分发 ⇒ 版本漂移的唯一提醒点就在这行字上（⛔ 不做自动校验命令） |
| 3 | skill 里**禁止**写「自动接受 / 批量放行」这类指令 | 人签是 ADR-005 的锚点，不能让 agent 替人签 |
| 4 | 现成的 `skills/ontok-extract/SKILL.md` **不重写**，只补上面两处 | 已有内容经受过实测（泛动词 3.60%→0），不动它 |
| 5 | `skills/ontok-build/SKILL.md`（含「盘点盲区」§一·五）也补同样的收口行 | 两条 skill 都会走到同一个出口 |

---

## 六 · 不做（划清边界）

| ⛔ | 理由 |
| --- | --- |
| 自动 apply / 自动 freeze | 人签是唯一锚点（ADR-005 / REQ-107） |
| 把 console 打包进构建包 | 审核台是本机进程，不是交付物 |
| 在服务端起这个审核台 | 那是 REQ-159 的上传确认流程，不是本地构建审核；两者**别混**（见 §八） |
| 支持「后台常驻 daemon」 | v1 前台运行，退出即失效，少一个常驻攻击面 |
| 重写 `review serve`（REQ-111） | 它审的是「提案双轨」，console 审的是「全流程」；两者并存，不合并 |

---

## 七 · 验收标准

### AC-001 · 起得来、打得开、要口令
- `ontok-builder review open --project <p>` ⇒ 打印 URL 与一次性口令；浏览器打开页面
- `GET /` 匿名 200；`GET /api/projects` 无 token ⇒ **401**；带 token ⇒ 200
- 判据：三条 curl + 页面可交互

### AC-002 · URL 里没有口令
- 打印的 URL 与浏览器地址栏**都不含** `token=` / `key=`
- 判据：抓进程 stdout 与浏览器实际 URL 两处

### AC-003 · 端口冲突自动顺延
- 先占住 8899，再跑 `review open` ⇒ 仍能起来，且打印的端口**不是** 8899
- 判据：一次实测 + 打印的 URL 可访问

### AC-004 · 退出即失效
- Ctrl-C 后：进程不在；同一 token 不再能调（服务已停）；token **没有落到任何文件**
- 判据：退出后 `Test-Path` 临时目录无 token 文件 + 全仓 `grep <token 前 8 位>` = 0

### AC-005 · 只绑回环
- 默认 `--bind 127.0.0.1:0`；给 `0.0.0.0:8899` 而不加 `--allow-public` ⇒ **启动即失败**（沿用 console 纪律）
- 判据：两条命令的退出码

### AC-006 · acp 模式一条命令到审核台
- `build run --agent acp ...` ⇒ 一次运行后：断言 JSONL、提案、版本都产出，且审核台已起
- 中途失败（agent 连不上）⇒ **退出码非 0 且零产物**（不产出断言文件、不建版本）
- 判据：成功/失败各一次，看产物与版本数

### AC-007 · ⛔ 不自动签
- 跑完 `build run` 后：新版本存在但 **未冻结**、提案条目**决策为空**
- 判据：`review show` 输出 + `ontology versions`

### AC-008 · skill 与收口一致（**人分发**，不验导出）
- `skills/ontok-extract/SKILL.md` 与 `skills/ontok-build/SKILL.md` 末尾都有收口命令 `ontok-builder review open`
- 两份 skill 头都写了「适用工具版本」
- 两份 skill 里**没有**「自动接受 / 批量放行 / 跳过审核」这类指令（grep 三条）
- 判据：grep 结果 + 人工读一遍收口段

---

## 八 · 与 REQ-159 的关系（别混）

| | REQ-160（本需求） | REQ-159 |
| --- | --- | --- |
| 审核台在哪 | **FDE 本机**（`review open` 起的 console） | **服务端**页面（上传构建包后） |
| 谁签 | FDE 本人 | 服务端上的操作人 |
| 产物 | 已冻结的本体 / 提案决策 | 一包 + 一个可查的 key |
| 顺序 | **先**（本地构建确认） | **后**（打包上传） |

⇒ 两条流程串起来才是完整交付：**本地抽 → 本地人签 → `bundle pack` → 服务端上传 → 服务端确认 → 出 key**。

---

## 九 · 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 一次性 token 打印到终端，被 shell 历史/截图带走 | 打印时**明确标「本机一次性口令，勿复制到共享文档」**；TTL 默认 4h；退出即失效 |
| 2 | `build run` 把四个阶段耦死，某一阶段想单独重跑 | `--work-dir` 保留中间产物；每阶段仍可单独用原命令重跑（编排只是**便利入口**，不是唯一入口） |
| 3 | agent 模式里 agent 不按 skill 收口（跑完就停） | skill 末尾写死收口命令 + 输出里提示「下一步：review open」；⛔ 不靠 agent 自觉做签 |
| 4 | 自动开浏览器在 SSH / 无桌面环境失败 | `--no-browser` 显式开关；开不出来**只警告不失败**（服务仍在） |
