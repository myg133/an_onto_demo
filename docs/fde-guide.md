# FDE 构建指南 · 用文档（和库）造一套本体

> **这份是给人读的**（FDE / 实施工程师）。它**不是 skill** —— 别装进 agent 的 skills 目录。
>
> | 目录 | 放什么 | 谁读 |
> | --- | --- | --- |
> | `Deploy/skills/` | agent 加载的 skill（有 `name`/`description` frontmatter） | **agent** |
> | `Deploy/docs/`（本文件） | 给人看的流程/运维文档 | **人** |
>
> 抽取 agent 要的是 [`../skills/ontok-extract/SKILL.md`](../skills/ontok-extract/SKILL.md)；
> 数字员工要的是 [`../skills/ontok-query/SKILL.md`](../skills/ontok-query/SKILL.md)。
> 本文里出现的命令都是**你**在终端跑，⛔ 不是给 agent 的指令。

> **适用工具版本**：`ontok-builder` **≥ v0.2.9**（本文件 2026-10-27 更新）。
> ⚠️ **构建类命令在 REQ-152 之后从 `ontok` 挪到了 `ontok-builder`** —— 旧材料里的 `ontok doc ingest` / `ontok ontology export` 等写法已失效。
> 命令对不上时**先升级工具**，别照着旧材料猜参数。

> 你是 FDE。这份**指南**的目标是：**让一套本体从"客户给的资料"变成"数字员工能用的 TTL + 接口"**。
> 每步都写了「**怎么验**」——⛔ 不要跳过验证，这套东西最容易出现"看起来成功了其实错了"。

---

## 〇 · 适用模式（**这是一份给人看的流程手册**）

> ✅ **ACP 模式要读** · ✅ **独立 agent 模式也要读** —— 两种模式下读它的人都是 **FDE**，不是 agent。
> ⛔ 不要把本文件当抽取契约塞给 agent：契约是 `ontok-extract`（**只有一份**，两模式共用）。

| 模式 | 谁在跑 | 本文件怎么用 | 抽完之后谁起评审台 |
| --- | --- | --- | --- |
| **ACP 模式** | `ontok-builder build run --agent acp --agent-command <cli> --agent-skill <ontok-extract 路径>` **主动驱动**外部 agent | FDE 读它：选 A/B 路、照「怎么验」验数、按「失败长相」排查 | **工具自动**起（四段跑完直接进 §三·六，无口令） |
| **独立 agent 模式** | 用户自己开 Claude Code / Codex / 其它 agent | FDE 读它（同上）；抽取契约由**人**装进那个 agent 的 skills 目录 | **人手动**跑 `console serve --dir <工作目录>`（§三·六） |

三条不变的事实：

| # | 事实 |
| --- | --- |
| 1 | 抽取契约**只有一份**（`ontok-extract`）—— ⛔ 别按模式拆成两份，拆了必漂移 |
| 2 | 模式差别只有三处：**skill 怎么进 agent** · **谁起评审台** · **谁保证阶段顺序** |
| 3 | 签字 / 定版 / 上传 Studio **永远是人**（两种模式都一样） |

> 时序图与「谁负责什么」：`MODES.md`。

---

## 〇·五 · 连接信息由部署注入（本文件不含地址与口令）

| 变量 | 用途 |
| --- | --- |
| `ONTOK_DATABASE_URL` | **元数据库**（存本体快照） |
| `ONTOK_SOURCE_DATABASE_URL` | **源库**（只在「有库」路径用；只读账号） |
| `ONTOK_LLM_BASE_URL` / `ONTOK_LLM_API_KEY` / `ONTOK_LLM_MODEL` | 抽取用的模型三件套 |

⛔ 口令不进任何文件、不进 skill、不进截图。

⚠️ **纯本地评审（§三·六）不需要 `ONTOK_DATABASE_URL`** —— 评审台只读工作目录里的文件。
只有「旧 DB 链」（`proposal apply` / `version freeze` / `ontology export --project …`）才要连元数据库。

---

## 一 · 开工前先确认三件事

| # | 确认什么 | 为什么 |
| --- | --- | --- |
| 1 | **有没有可连的关系库？** | 决定走 **A** 还是 **B**（两条路差别很大） |
| 2 | **文档是什么格式？** | ⛔ 只吃 `.md` / `.txt`（UTF-8）。PDF/Word 先外部转文本，**记下转换后的 hash** |
| 3 | **LLM 三件套在不在？** | 缺了会在摄取阶段失败；先 `echo $ONTOK_LLM_MODEL` 确认 |

### 两条路

| | A · 有库 | B · 纯文档 |
| --- | --- | --- |
| 输入 | 库 + 可选文档 | 只有文档 |
| 主要证据 | **值域重叠实测**（关系可信） | **原文逐字引用**（概念有出处） |
| 关系可信度 | 高（有列可验） | 低（只有语义） |
| 典型客户 | 有业务库 | 只有手册/规范/巡检文档 |

**B 不是"降级版"**——它只是换了证据类型。但要提前跟客户说清：**B 造出来的关系是"文档级语义"，不是"数据验证过"**。

---

## 一·五 · 先进「盘点盲区」这一环（⛔ 跳过它，你会建出一套"看着全、其实漏钱"的本体）

### 为什么这一环必须有

**实测**：某套 30 类本体上线后，数字员工拿账单交叉核对，发现——

> **轻量应用服务器占当月支出 14.96%（382 元），但本体里没有这个类** ⇒ 这部分钱在系统里**是隐形的**。
> 同类还有 **NAT / EIP / VPN 网关 / 云联网 / COS**：都在产生费用，都没有类。

**问题不在"抽得不好"，在"没抽对范围"。** 抽取从文档出发，而**该建哪些类应该由"钱和资源"决定**。

### 三个输入

| 输入 | 从哪来 | 告诉你什么 |
| --- | --- | --- |
| **账单按产品汇总** | 云厂商的 `…BillSummaryByProduct` 一类接口 | 哪些产品**真的在花钱**（含占比） |
| **资源清单** | 已有契约的类实际返回的对象，按产品分组 | 哪些产品**真的在用** |
| **现有类 + 契约** | `GET /api/catalog` + `am-<source>.json` | 现在**覆盖了什么** |

### 三种缺口（三张表对比就出来）

| 缺口 | 长相 | 后果 | 优先级 |
| --- | --- | --- | --- |
| **有消费/有资源，但没有类** | 账单里有产品，`catalog` 里找不到对应类 | **隐形支出**（钱花了，系统里查不到） | **按金额排** |
| **有类，但没有取数契约** | `catalog` 有，`am-*.json` 没有 | 查它只会 `503`——像"没有这东西" | 按被问到的频次 |
| **契约引用了不存在的类** | `am-*.json` 有，`catalog` 没有 | **部署直接起不来** | P0，先修 |

### ⛔ 产品名 ↔ 本体类 的映射**必须声明，不许让工具猜**

账单里叫"轻量应用服务器"，本体里可能叫 `lighthouse_instance`，也可能你想把它归进 `cvm_instance`。
**这是业务决策，不是字符串匹配。**

⇒ 写一张**字典**（产品名 → 类 key，或 `null` 表示明确不建），**人签**；工具只按字典对账。

> 与既有原则一致：**机制 / 声明 / 决策**三层分开（ADR-006）。工具做机制，字典是声明，归不归并是决策。

### 这一环的产物

```
gap-report.md
├─ missing_classes[]            按金额降序（产品 / 金额 / 占比 / 建议类 key / 决策人）
├─ classes_without_contract[]   有类但无契约（附"被问过几次"）
└─ contract_without_class[]     契约指向不存在的类（P0，先修）
```

**把 `missing_classes` 里"决定要建"的，喂给下一轮：采语料 → 抽取 → 提案。**

### 什么时候做这一环

| 时机 | 为什么 |
| --- | --- |
| **接一个新客户/新环境** | 第一次必须盘，否则范围就是猜的 |
| **客户新增了产品** | 账单会先变，本体不会 |
| **巡检发现"数字对不上"** | 往往是漏了一类（如上面那 15%） |
| **每季度复核一次** | 产品线在变，本体要跟上 |

> ⛔ **不要"抽完文档就算完"**：文档只覆盖你采过的产品；**账单覆盖客户真正在用的**。

---

## 二 · 路径 A（有库）· 六步

```bash
# 1 勘查（只读，不搬数据）
ontok-builder introspect --out work/schema.json
#   ✅ 验：表数 / 列数 / 有注释的比例
#      空库或全无注释 ⇒ 先回去问客户"这套库是不是对的"

# 2 生成翻译表（表名中文名）—— **--names 现在是必填**
ontok-builder name suggest --schema work/schema.json --out work/names.json --as-names work/table_names.json
#   ✅ 验：覆盖了多少张表（覆盖不全 ⇒ 后面实体名会是英文/占位）

# 3 提案（关系 + 命名）
ontok-builder propose --schema work/schema.json --names work/table_names.json \
              --out work/proposal.json --fk-out work/fk-proposals.json
#   ✅ 验：关系条数、strong / medium / 否决 各多少；样本命中率

# 4（可选）用文档补充语义
ontok-builder doc ingest --input docs/ --out work/assertions.jsonl --stats work/doc-stats.json
ontok-builder doc merge  --assertions work/assertions.jsonl --proposal work/proposal.json \
                 --schema work/schema.json --out work/merged.json --stats work/merge-stats.json
#   ✅ 验：并入 / 排除不可对齐 / 未映射 / 冲突 五项

# 5 人签（⛔ 不可自动化）—— console v2 本机评审台，见 §三·六
#   ① 草稿放到 <工作目录>/out/proposal.json（评审台只读这一个路径）
#      有文档则用 work/merged.json，纯库路径用 work/proposal.json —— 复制过去即可
#   ② 起评审台：一个工作目录 = 一个项目，无口令、无项目参数、不连库
ontok-builder console serve --dir <工作目录>
#   👤 人在页面上做四件事：逐条两轨签（业务/建模）→ 定版 → 导出构建包

# 6 导出（**导出的是定版**，不是草稿）
#   6a 主路径：页面上点「导出构建包」→ 从定版快照打包
#       产物 <工作目录>/.ontok/bundles/<目录名>-v<N>.ontokbundle（包内快照与定版逐字节相同）
# 无 UI / CI 里出同一个包（与页面导出**逐字节相同**；--built-at 钉死才可比 sha）
ontok-builder bundle pack --dir <工作目录> [--version v1] --out <路径> [--built-at <RFC3339>]
ontok-builder bundle verify --file <工作目录>/.ontok/bundles/<目录名>-v<N>.ontokbundle
#   ✅ 验：exit 0（2=包不合规 / 1=环境问题）
#   6b 附加：部署端还要 TTL / R2RML 时（**旧 DB 链**：要连元数据库、按项目/版本）
ontok-builder ontology export --project <项目> --version <N> --out out/customer.ttl
ontok-builder ontology export --project <项目> --version <N> --model-view \
                      --schema work/schema.json --tenancy work/tenancy.json --out out/model.ttl
ontok-builder r2rml generate --project <项目> --version <N> \
                      --schema work/schema.json --out out/mapping.ttl
#   ✅ 验：类 / 关系 / 属性 三个数；TTL sha256
```

> ⚠️ **别两套并行**：旧 DB 链（`proposal import` → `proposal apply` → `version freeze` → `ontology export --project/--version`）只用于**仍有元数据库的部署**；
> 纯本地工作目录用 console v2 的「签 → 定版 → 导出包」。⛔ 同一份本体不要两边各签发一次。

---

## 三 · 路径 B（纯文档）· 五步

```bash
# 1 摄取（要 LLM；按批计 token）
ontok-builder doc ingest --input docs/ --out work/assertions.jsonl \
                 --stats work/doc-stats.json --budget-tokens 500000
#   ✅ 验：断言数 / 丢弃（quote 找不到）条数 / 用了多少 token
#   ⚠️ 超预算 ⇒ 一条断言都不产出（设计如此，"不产出半份"）

# 2 合并成提案 —— **必须带 --allow-doc-only**（否则空提案会把全部断言判为未映射）
ontok-builder doc merge --assertions work/assertions.jsonl --proposal work/empty-proposal.json \
                --allow-doc-only --out work/merged.json --stats work/merge-stats.json
#   ✅ 验：新建实体数 / 未映射 / 冲突
#   ⚠️ 空提案文件内容：{"version":"1.0.0","items":[]}

# 3 人签（⛔ 不可自动化）—— console v2 本机评审台，见 §三·六
#   草稿：把 work/merged.json 放到 <工作目录>/out/proposal.json（`doc merge` 的产物就是这个形状）
ontok-builder console serve --dir <工作目录>
#   👤 人在页面上：逐条两轨签（业务/建模）→ 定版 → 导出构建包

# 4 导出（B 没有 schema ⇒ 不进库、也不导 TTL；**只要那个定版包**）
#   页面上「导出构建包」→ .ontok/bundles/<目录名>-v<N>.ontokbundle
# 无 UI / CI 里出同一个包（与页面导出**逐字节相同**；--built-at 钉死才可比 sha）
ontok-builder bundle pack --dir <工作目录> [--version v1] --out <路径> [--built-at <RFC3339>]
ontok-builder bundle verify --file <工作目录>/.ontok/bundles/<目录名>-v<N>.ontokbundle
#   ✅ 验：包内快照 == .ontok/versions/v<N>/snapshot.json（逐字节）；exit 0

# 5 ✅ 验（定版里）：类数 = 断言里的概念数（去重后）；关系数 = 两端都能解析的关系数；
#   被 ✗ 拒的条目**不在**这一版里（也不在包里）
```

### ⚠️ 路径 B 的两个常识

**① 概念会很多**

原始断言是"原文片段级"的。728 条断言可能抽出 **506 个概念**——直接全建会得到一个没法用的本体。

**建议**：先用 `--budget-tokens` 控制范围，或**只喂核心文档**（规范/流程），别一开始就灌全部会议纪要。

**② 关系很脆**

文档里的关系是"动词"（"使用"、"绑定"、"降级"）。两端名字必须在概念集合里**精确对得上**——差一个字就进 `doc_unmapped`。

---

## 三·五 · 抽取用哪个实现：内置 LLM vs **ACP + skill**（有实测）

**同一个模型**（`deepseek-v4.1-flash`）、**同一份语料**（3 篇 / 39 块）：

| 指标 | 内置 LLM | **ACP + skill** |
| --- | ---: | ---: |
| 断言 / 关系 | 397 / 139 | **593 / 258** |
| **泛动词占比** | 3.60% | **0.00%** |
| **关系两端都是概念** | 65.5% | **89.9%** |
| 耗时 / token | 7.1 min / 110K | **4.7 min** / 187K |

```bash
ontok-builder doc ingest --input docs/ --out a.jsonl --stats s.json \
  --agent acp --agent-command "pi-acp" --agent-skill skills/ontok-extract/SKILL.md
```

> ⚠️ 这条 `--agent …` 就是 **ACP 模式**：工具主动驱动外部 agent，`--agent-skill` 把 `ontok-extract` 的**路径**交给对端。
> **独立 agent 模式没有这条命令** —— 你自己开 agent、自己把 skill 装进它的 skills 目录（见 §〇 与 `MODES.md`）。
> 两种模式**产出同一份契约**（概念/关系/逐字证据），所以这张表对两种模式都成立。

**怎么读**：

| # | 判读 |
| --- | --- |
| 1 | 效果**真实但不神奇**：泛动词清零、端点闭合 **+24pp**、关系数 +86% |
| 2 | 关系变多不是"抽得更全"，是**更愿意把结构写成边**（`包含字段` 101 条 vs 内置 24 条） |
| 3 | 代价 **token +71%**（skill 全文随每块下发），但**墙钟反而快 34%**（没有解析失败要重试） |
| 4 | ⚠️ **单次运行（n=1）**，没测方差 |
| 5 | ⛔ **旧材料里「泛动词降 93%、端点闭合 26%→96%」那组不得引用**——它混了模型变量（ACP 侧 MiniMax / 内置侧 deepseek），跨实验还换了语料，不可比 |

> 要重新测：`BA/demands/REQ-147/evidence-same-model.md` 有完整复现命令与判读。

### ⭐ 定版前必须跑：关系动词校验（REQ-157）

```bash
ontok-builder relation verbs check --project <项目> --version <版本>
```

| 退出码 | 含义 | 能不能定版 |
| ---: | --- | --- |
| 0 | 干净；或用到的只是别名（自动归一） | ✅ 能 |
| **4** | 有 `forbidden` 泛词 或 `unknown` 字典外动词 | ⛔ **不许定版** |

- `alias_normalized`（如 `attaches_to`）与 `deprecated`（如 `contains`）**只是提示**，不阻塞
- 报告给两份：stderr 人话 + stdout JSON（带 `occurrences[].location / from / to`）
- 动词字典是仓库根的 `relation-verbs.json`，**加动词要人审**

> **为什么必须跑**：v3 实际就有 `uses` / `associated_with` 两个泛词混进了定版 —— 那时还没有这道闸。

---

## 三·六 · 审核台 = **console v2 本机评审台**：人签**在这一步**发生（⛔ 不许跳过、不许代签）

> 抽取是机器做的，**签字只能是人做的**（ADR-005）。这一步就是那个「人」的入口。
> 一句话状态模型：**agent 出草稿 → 人逐条签 → 定版 → 导出的是定版**。

### 1 · 起法（无口令、无项目、无库）

```bash
# 一个工作目录 = 一个项目；--dir 指到它
ontok-builder console serve --dir <工作目录> [--bind 127.0.0.1:8899] [--no-browser]
```

| 参数 | 说明 |
| --- | --- |
| `--dir` | **工作目录**（默认 `.`）。目录名 sanitize 后就是项目名（进包名 / 页面标题） |
| `--bind` | 默认 `127.0.0.1:8899`。**只许回环**：非回环地址**启动即拒**（没有放行开关） |
| `--no-browser` | 只影响开不开浏览器，不影响起不起来（SSH / 无桌面用） |

⛔ v2 里**没有**这四类参数：口令表 · 项目作用域 · 声明校验 · 放行非回环。写了直接报参数错。
✅ 不需要 `ONTOK_DATABASE_URL`：评审台**一行 SQL、一次网络都没有**（AC-007 是硬条件）。

### 2 · 工作目录里只有四个落点（定版占两个文件；全在你本地）

```
<工作目录>/out/proposal.json                        草稿（agent 产出，**只读**：页面不改它一个字节）
<工作目录>/.ontok/decisions.json                    人的裁决（两轨 + 理由；唯一可写状态）
<工作目录>/.ontok/versions/v<N>/snapshot.json       定版快照（**冻结，不可改**）
<工作目录>/.ontok/versions/v<N>/meta.json           {seq, created_at, items, snapshot_sha256, proposal_sha256}
<工作目录>/.ontok/bundles/<目录名>-v<N>.ontokbundle 导出包（**只含定版**）
```

### 3 · 页面上只有四个动作（顺序不能换）

| # | 动作 | 在哪 | 判据 |
| --- | --- | --- | --- |
| 1 | **读草稿** | 左栏「评审（核心）」 | 看到条数 / 两轨进度；草稿缺失 ⇒ 页面提示先产出 `out/proposal.json` |
| 2 | **逐条签** | 每条右侧 ✓ / ✗（**业务轨 + 建模轨都要签**），另有「全选接受 / 全选拒绝」 | 两轨都签 = 该条 signed；**只签一轨仍是 pending** |
| 3 | **定版** | 右栏「定版」按钮 | 全部签完才能点；生成 v(N+1) 的 `snapshot.json` + `meta.json` |
| 4 | **导出包** | 右栏「导出构建包」+ 下载 | 只从定版打包；包内快照与 `versions/vN/snapshot.json` **逐字节相同** |

### 4 · 门禁：点不动就是没签完，**不是 bug**

| 门禁 | 表现 | 为什么 |
| --- | --- | --- |
| **未签完 ⇒ 不能定版** | 「定版」报 `DRAFT_NOT_READY`（还有 pending，零产物） | 未签的条目 = 没人负责的结论 |
| **未定版 ⇒ 不能导出** | 「导出构建包」报 `NO_VERSION` | 导出的是**定版**，不是草稿、不是页面当前状态 |
| **草稿变了 ⇒ 旧签字过期** | 顶部亮 stale 横幅；定版报 `DECISIONS_STALE`（409）；**重签任意一条**即认当前草稿 | 人签的是"这一版草稿"，不是"随便哪版草稿" |

### 5 · ⛔ 定版之后不可改（要改就**再定一版**）

- 定版 = **冻结**：`versions/vN/snapshot.json` 此后**不再变**；草稿再改也**不回头改旧定版**
- 要改 ⇒ 在草稿上继续签 → 再点「定版」→ 得到 **v(N+1)**；旧的 vN 仍在（可追溯，可导出老包）
- 定版号 = `max(已有 vN) + 1`；v1 的 base 是空快照，v(N+1) 的 base 是 vN
- ✗ 拒掉的条目**只是不进这一版**（不是删了）——草稿和 `decisions.json` 里都还在
- ⛔ 不许手改 `out/proposal.json` 或 `.ontok/decisions.json` 绕过评审：评审记录是唯一"谁认可了什么"的证据

### 6 · ⛔ 三条禁令（外加一条前置检查）

| # | 禁令 | 为什么 |
| --- | --- | --- |
| 1 | ⛔ **agent 不许替人签**（不许调 `decide`、不许点「全选接受」） | 签字的语义是「这个人看过并负责」 |
| 2 | ⛔ **agent 不许替人定版**（`finalize` 只能人手点） | 定版是 ADR-005 的锚点；agent 到「草稿」为止（见 `ontok-extract` §十） |
| 3 | ⛔ **不许手改文件冒充签字 / 跳过评审直接定版** | 未签的条目进查询上下文 = 未经人确认的自动化结论 |

**前置检查**：定版前**必须**跑 `relation verbs check`（`exit 4` ⇒ 不许定版）—— 完整说明只在 §三 末节写一遍，这里不重复。

### 7 · ⛔ 旧审核台（**不要再用**）

| 旧命令 | 是什么 | 现状 |
| --- | --- | --- |
| `review open --project <项目>` | **旧：DB 版审核台**（REQ-160；一次性 admin token + 连元数据库 + 建项目/版本） | **已被 console v2 取代**，别再当主路径 |
| `review decide --accept-all …` | **旧：DB 版批量放行** | **已被取代**：v2 里没有这个概念，逐条签就是逐条签 |
| `console serve` + 口令表 / 项目作用域 / 声明校验 | **旧：带口令 / 项目 / 声明的管理台** | **已被取代**：那些参数已删，写了直接报错 |

> ✅ REQ-163 起，`build run` 的尾步**就是** console v2（同一份启动实现），**不再**落到旧 `review open`。

### 8 · 定版之后：出包 →（**人工**）传 Studio

```bash
# 页面上点「导出构建包」= 从**定版快照**打包（复用 bundle::pack），不重算、不重新抽
# 产物：<工作目录>/.ontok/bundles/<目录名>-v<N>.ontokbundle
# 无 UI / CI 里出同一个包（与页面导出**逐字节相同**；--built-at 钉死才可比 sha）
ontok-builder bundle pack --dir <工作目录> [--version v1] --out <路径> [--built-at <RFC3339>]
ontok-builder bundle verify --file <工作目录>/.ontok/bundles/<目录名>-v<N>.ontokbundle
#   0 = 通过 · 2 = 包不合规 · 1 = 环境问题
```

| 项 | 事实 |
| --- | --- |
| **导出的是定版** | 不是草稿、不是"页面当前状态"、不是重新算一遍；包内 `snapshot.json` 与定版快照**逐字节相同** |
| **上传是人工的** | console **不上传、不入库、不激活**（那些是 Studio 的事，ADR-013）；把包交给 Studio 的人去传 |
| 包里是**入库快照原文** | 不是 `ontology show` 的视图（视图只有 5 个字段，证据会丢） |
| `am/` 只校验不落库 | P1a 服务端对包里的 `am/` **只校验不落库** ⇒ 传上去也不会立刻生效（看 import 回执 `warnings[]`） |

---

## 四 · 失败长相（**这一节最重要**）

| 现象 | 真实原因 | 怎么办 |
| --- | --- | --- |
| 断言 **0 条** | 文档不是 UTF-8 / 全是 PDF / 目录为空 | 检查编码与扩展名；PDF 先转 |
| 断言有，但合并后**并入 0** | 走了 A 的合并却没有库实体 | 纯文档必须 `--allow-doc-only` |
| **TTL 里关系是 0** | 关系被 schema 对账吃掉（没有可用的列映射） | 检查 `excluded_relations`；文档类本体本来就容易 0 |
| **catalog 里类少一半** | 部署的版本不对 / 项目名写错 | `health` 里看 `project` + `ontology_version` |
| 部署后**接口 503** | 部署是本体模式（`--ontology-only`），而你以为有数据 | 有库的部署**不能**加 `--ontology-only` |
| 查询 **500 TENANT_UNRESOLVED** | 表在本体里但没登记进 `query-policy.json` | 补声明，或把表排除后重新发布 |
| 审计里 `tenant=00000000-…` | token 里填了假租户 | 本体模式**不许**填 tenant（会被拒启）；普通模式必须用真实租户 |
| **EXTRA**：实体名全变"对象 001" | 用了脱敏占位翻译表 | 必须用自己 schema 生成的 `table_names.json` |
| **导出被拒**，报"发布闸"/敏感词 | 概念描述里出现内部术语（如 `evidence`）⇒ **整份拒绝**，不是跳过一条 | 改写该描述；原文仍在 `doc_evidence[]` 里，不丢 |
| 纯文档路径里**实体名重复** | 某概念的**别名 = 另一概念的名字**（如 `CLI 直采` 与 `CLI`） | 已按「名字优先于别名」解析；若仍冲突，说明断言本身有歧义，回去看原文 |

**console v2 评审台的四条「长相」**（都是门禁在干活，不是 bug）：

| 现象 | 真实原因 | 怎么办 |
| --- | --- | --- |
| 「定版」点不动 / 报 `DRAFT_NOT_READY` | 还有条目 **pending**（业务轨与建模轨**都要签**） | 回去逐条签；⛔ 别用脚本刷裁决冒充签字 |
| 「导出构建包」报 `NO_VERSION` | 还没定版 | 先定版（§三·六）；导出的是定版，不是草稿 |
| 定版报 `DECISIONS_STALE`（409） | 草稿在签的过程中被改过（sha 不符） | 重签**任意一条**（= 认当前草稿）再定版；已签的不会丢 |
| 页面打不开 / 端口占用 | `--bind` 的端口被别的进程占了 | 换端口，或 `--bind 127.0.0.1:0` 让内核分配（横幅会打真实端口） |

---

## 四·五 · 当有些数据来自 API（不是库）

如果一类数据在云上/第三方接口里，不在客户库里：

```
本体里照常建这个类（它有语义地位）
映射另写一份：access mapping（哪个接口、哪些参数、响应哪一段）
```

**三件必须知道**：

| # | 事项 |
| --- | --- |
| 1 | 这类类在 TTL 里会带 `ontok:sourceKind "api"`（**客户视图也带**——让下游知道"平台不执行它"） |
| 2 | 映射**与本体同版本发布**（`mapping_version` 与 `ontology_version` 分开记） |
| 3 | 下游拿到的查询结果里，这类数据会出现在 `@pending` 里（半成品）——**这是设计，不是缺陷** |

⛔ **不要把 API 的端点/参数写进本体 TTL** —— 那些只在 access 映射里；

## 五 · 禁忌

0. ⛔ **不盘点就开建** —— 文档只覆盖你采过的产品，账单才覆盖客户真在用的；漏一类可能是 15% 的支出。

1. ⛔ **跳过 `--schema` 的校验**（有库却不给它，`rdflib` 类型全变启发式）
2. ⛔ **手改 `proposal.json` 绕过评审**——评审记录是唯一"谁认可了什么"的证据
3. ⛔ **把推断的字段类型当事实**（TTL 里会标 `rangeSource: heuristic`，被问就如实说）
4. ⛔ **用假数据填充**（合成快照必须标 `provider: synthetic`）
5. ⛔ **给客户承诺"关系是验证过的"**——路径 B 的关系只有文档语义
6. ⛔ **在规则不清时开会话**（先跑通一个小闭环，再谈全量）

---

## 六 · 发布到环境

> **主路径（console v2）**：`导出构建包` → 把 `.ontok/bundles/<目录名>-v<N>.ontokbundle` **人工**传到 Studio。
> ⛔ console **不上传、不入库、不激活**；上传是人的动作，不是工具的动作（ADR-013）。
> ⚠️ 传上去之后包里的 `am/` 只校验不落库（P1a），要生效走 Studio 的导入流程。

**下面这条是「有元数据库的部署」的旧链路**（也能用，但它不是纯本地构建的主路径）：

```bash
# 1 把本体推到集群元数据库（如果构建在本地）
#    注意：TTL 是确定性的 —— 同一份 proposal 在另一台库重建，TTL 应逐字节相同
ontok-builder ontology export --project <项目> --version <N> --out out/customer.ttl

# 2 部署形态二选一
#    A 有库 ⇒ 普通模式（--schema + --policy + 源库）
#    B 纯文档 ⇒ 本体模式（--ontology-only，不需要 schema/policy/源库）

# 3 发布后必验四条
GET /api/health        → project / ontology_version / mode 对不对
GET /api/catalog       → 类/关系/属性数与导出时一致
GET /api/ontology.ttl  → sha256 与本地导出**逐字节相同**
POST /api/sparql       → 有库应返回 JSON-LD 图；纯文档应 503 NO_DATA_SOURCE
```

**文档一起发布**（可选但建议）：把原始 `.md` 放到只读 PVC/对象存储，用于**溯源**——将来有人问"这条关系凭什么"，能翻到原文。

---

## 七 · 一页检查单

> ⛔ **第 0 条：做过「盘点盲区」了吗？**（§一·五）没做过 ⇒ 这套本体的范围是猜的。

```
□ 有库吗？→ A / B 选一条
□ 文档是 .md/.txt 且 UTF-8？
□ ONTOK_LLM_* 三件套在？
□ --names 显式给了真实翻译表？
□ 纯文档走了 --allow-doc-only？
□ 五项合并统计看过？（并入/排除/未映射/冲突/doc-only）
□ 草稿落在 <工作目录>/out/proposal.json 了吗？（评审台只读这一个路径）
□ 审核台起了吗？（console serve --dir <工作目录>；§三·六，无口令）
□ 人签是真的人签的吗？（两轨**都**签；⛔ 不是 agent 代点「全选接受」）
□ 签完才定版的吗？（未签完 ⇒ DRAFT_NOT_READY，零产物）
□ 定版前跑过 relation verbs check 了吗？（exit 4 ⇒ 不许定版）
□ 定版号记住了吗？（.ontok/versions/v<N>/；**定版后不可改**，要改再定一版）
□ 导出的是**定版**吗？（包内快照 == versions/v<N>/snapshot.json，逐字节）
□ bundle verify 过了吗？（exit 0）
□ 包是**人工**传 Studio 的吗？（console 不上传、不入库、不激活）
□ （有库部署才需要）导出 TTL 的 sha256 记了？发布后四条验证过了？
```
