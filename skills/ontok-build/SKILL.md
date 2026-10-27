---
name: ontok-build
description: FDE 用：把客户文档（和/或关系库）变成一套可发布的本体 —— 从摄取、合并、人签、冻结到导出 TTL 与发布。含每步的验证方法与「失败长相」对照表。不涉及消费侧调用（那见 ontok-query）。
category: workflow
---

# ontok-build · 用文档（和库）造一套本体

> **适用工具版本**：`ontok-builder` **≥ v0.2.9**（本文件 2026-10-27 更新）。
> ⚠️ **构建类命令在 REQ-152 之后从 `ontok` 挪到了 `ontok-builder`** —— 旧材料里的 `ontok doc ingest` / `ontok ontology export` 等写法已失效。
> 命令对不上时**先升级工具**，别照着旧材料猜参数。

> 你是 FDE。这份 skill 的目标是：**让一套本体从"客户给的资料"变成"数字员工能用的 TTL + 接口"**。
> 每步都写了「**怎么验**」——⛔ 不要跳过验证，这套东西最容易出现"看起来成功了其实错了"。

---

## 〇 · 连接信息由部署注入（本文件不含地址与口令）

| 变量 | 用途 |
| --- | --- |
| `ONTOK_DATABASE_URL` | **元数据库**（存本体快照） |
| `ONTOK_SOURCE_DATABASE_URL` | **源库**（只在「有库」路径用；只读账号） |
| `ONTOK_LLM_BASE_URL` / `ONTOK_LLM_API_KEY` / `ONTOK_LLM_MODEL` | 抽取用的模型三件套 |

⛔ 口令不进任何文件、不进 skill、不进截图。

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

# 5 人签（⛔ 不可自动化）—— 逐条签在**审核台**上做，见 §三·六
ontok-builder project new --name <项目>
ontok-builder version new --project <项目>
ontok-builder propose import --file work/merged.json
ontok-builder review open --project <项目>     # REQ-160 起；当前版本用 console serve（§三·六）
#   👤 人在页面上：「评审」tab 逐条签 → 「项目与版本」tab apply → 冻结
#   ⛔ 只有人明确说「全部接受」时才允许：
#      ontok-builder review decide --proposal p1 --project <项目> --accept-all
ontok-builder proposal apply --proposal p1 --project <项目>   # 人签完之后
ontok-builder version freeze --project <项目> --version 2

# 6 导出
ontok-builder ontology export --project <项目> --version 2 --out out/customer.ttl
ontok-builder ontology export --project <项目> --version 2 --model-view --schema work/schema.json \
                      --tenancy work/tenancy.json --out out/model.ttl
ontok-builder r2rml generate --project <项目> --version 2 --schema work/schema.json --out out/mapping.ttl
#   ✅ 验：类 / 关系 / 属性 三个数；TTL sha256
```

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

# 3 人签（⛔ 不可自动化）—— 逐条签在审核台上做，见 §三·六
ontok-builder project new --name <项目> && ontok-builder version new --project <项目>
ontok-builder propose import --file work/merged.json
ontok-builder review open --project <项目>     # REQ-160 起；当前版本用 console serve（§三·六）
#   👤 人在页面上逐条签 → apply → freeze
#   ⛔ 只有人明确说「全部接受」时才允许 review decide --accept-all
ontok-builder proposal apply --proposal p1 --project <项目>   # 人签完之后
ontok-builder version freeze --project <项目> --version 2

# 4 导出（B 没有 schema ⇒ 不带 --schema）
ontok-builder ontology export --project <项目> --version 2 --out out/customer.ttl

# 5 ✅ 验：类数 = 断言里的概念数（去重后）；关系数 = 两端都能解析的关系数
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

**怎么读**：

| # | 判读 |
| --- | --- |
| 1 | 效果**真实但不神奇**：泛动词清零、端点闭合 **+24pp**、关系数 +86% |
| 2 | 关系变多不是"抽得更全"，是**更愿意把结构写成边**（`包含字段` 101 条 vs 内置 24 条） |
| 3 | 代价 **token +71%**（skill 全文随每块下发），但**墙钟反而快 34%**（没有解析失败要重试） |
| 4 | ⚠️ **单次运行（n=1）**，没测方差 |
| 5 | ⛔ **旧材料里「泛动词降 93%、端点闭合 26%→96%」那组不得引用**——它混了模型变量（ACP 侧 MiniMax / 内置侧 deepseek），跨实验还换了语料，不可比 |

> 要重新测：`BA/demands/REQ-147/evidence-same-model.md` 有完整复现命令与判读。

### ⭐ 冻结前必须跑：关系动词校验（REQ-157）

```bash
ontok-builder relation verbs check --project <项目> --version <版本>
```

| 退出码 | 含义 | 能不能冻结 |
| ---: | --- | --- |
| 0 | 干净；或用到的只是别名（自动归一） | ✅ 能 |
| **4** | 有 `forbidden` 泛词 或 `unknown` 字典外动词 | ⛔ **不许冻结** |

- `alias_normalized`（如 `attaches_to`）与 `deprecated`（如 `contains`）**只是提示**，不阻塞
- 报告给两份：stderr 人话 + stdout JSON（带 `occurrences[].location / from / to`）
- 动词字典是仓库根的 `relation-verbs.json`，**加动词要人审**

> **为什么必须跑**：v3 实际就有 `uses` / `associated_with` 两个泛词混进了冻结版本 —— 那时还没有这道闸。

---

## 三·六 · 审核台：人签**在这一步**发生（⛔ 不许跳过、不许代签）

> 抽取是机器做的，**签字只能是人做的**（ADR-005）。这一步就是那个「人」的入口。

### 1 · 起审核台

**现在（v0.2.9 及以前）**

```bash
ontok console serve --bind 127.0.0.1:8899 \
  --tokens <admin-tokens.json> \
  --project <项目> --schema work/schema.json
# 浏览器打开 http://127.0.0.1:8899/ → 顶部填 admin token（口令只留在本标签页）
```

**REQ-160 落地后（更省事，推荐）**

```bash
ontok-builder review open --project <项目>
# 自动选空闲端口 + 生成一次性 admin token（不落盘）+ 自动开浏览器
# Ctrl-C 退出即失效；⛔ 口令不会进 URL
```

### 2 · 页面上要做的四件事（顺序不能换）

| # | 在哪 | 做什么 | 判据 |
| --- | --- | --- | --- |
| 1 | **评审** tab | **逐条**两轨签（业务轨 / 建模轨），看证据原文再决定 | `review show --proposal p1` 里 `signed == total` |
| 2 | **项目与版本** tab | 应用提案（apply）⇒ 生成**新版本** | 版本列表多一行、对象/关系计数非 0 |
| 3 | 同上 | 冻结（freeze） | `frozen_at` 非空 |
| 4 | **导出** tab 或 CLI | 导出 TTL / R2RML，记 sha256 | 与 CLI 导出**逐字节相同** |

### 3 · ⛔ 三条禁令（外加一条前置检查）

| # | 禁令 | 为什么 |
| --- | --- | --- |
| 1 | ⛔ **不许用 `--accept-all` 代替逐条签**（除非人明确说「全部接受」并要求记下理由） | 批量放行会让「人签」退化成形式，ADR-005 的锚点就没了 |
| 2 | ⛔ **agent 不许替人签**（agent 模式只到「起审核台」为止，见 `ontok-extract` §十） | 签字的语义是「这个人看过并负责」 |
| 3 | ⛔ **不许跳过评审直接 apply** | 未签的条目进查询上下文 = 未经人确认的自动化结论 |

**前置检查**：冻结前**必须**跑 `relation verbs check`（`exit 4` ⇒ 不许冻结）—— 完整说明只在 §三 末节写一遍，这里不重复。

### 4 · 签完之后（要把本体交给服务端时）

```bash
ontok-builder bundle pack --project <项目> --version <版本> --out out/<项目>-v<版本>.ontokbundle
ontok-builder bundle verify --file out/<项目>-v<版本>.ontokbundle   # 0=通过 2=包不合规 1=环境问题
```

⚠️ 包里 `snapshot.json` 是**入库快照原文**，不是 `ontology show` 的视图（视图只有 5 个字段，证据会丢）。
⚠️ P1a 服务端对包里的 `am/` **只校验不落库** ⇒ 传上去也不会立刻生效（看 import 回执的 `warnings[]`）。

---

## 四 · 失败长相（**这一节最重要**）

| 现象 | 真实原因 | 怎么办 |
| --- | --- | --- |
| 断言 **0 条** | 文档不是 UTF-8 / 全是 PDF / 目录为空 | 检查编码与扩展名；PDF 先转 |
| 断言有，但合并后**并入 0** | 走了 A 的合并却没有库实体 | 纯文档必须 `--allow-doc-only` |
| **TTL 里关系是 0** | 关系被 schema 对账吃掉（没有可用的列映射） | 检查 `excluded_relations`；文档类本体本来就容易 0 |
| **catalog 里类少一半** | 冻结的版本不对 / 项目名写错 | `health` 里看 `project` + `ontology_version` |
| 部署后**接口 503** | 部署是本体模式（`--ontology-only`），而你以为有数据 | 有库的部署**不能**加 `--ontology-only` |
| 查询 **500 TENANT_UNRESOLVED** | 表在本体里但没登记进 `query-policy.json` | 补声明，或把表排除后重新冻结 |
| 审计里 `tenant=00000000-…` | token 里填了假租户 | 本体模式**不许**填 tenant（会被拒启）；普通模式必须用真实租户 |
| **EXTRA**：实体名全变"对象 001" | 用了脱敏占位翻译表 | 必须用自己 schema 生成的 `table_names.json` |
| **导出被拒**，报"发布闸"/敏感词 | 概念描述里出现内部术语（如 `evidence`）⇒ **整份拒绝**，不是跳过一条 | 改写该描述；原文仍在 `doc_evidence[]` 里，不丢 |
| 纯文档路径里**实体名重复** | 某概念的**别名 = 另一概念的名字**（如 `CLI 直采` 与 `CLI`） | 已按「名字优先于别名」解析；若仍冲突，说明断言本身有歧义，回去看原文 |

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
□ 冻结前跑过 relation verbs check 了吗？（exit 4 ⇒ 不许冻结）
□ 审核台起过了吗？（§三·六；review open 或 console serve）
□ 人签是真的人签的吗？（signed == total，**不是 --accept-all 一把过**）
□ 冻结后版本号记住了？
□ 导出 TTL 的 sha256 记了？
□ 发布后四条验证都过了？
□ （要交给服务端时）bundle pack 打了、bundle verify 过了吗？（exit 0）
```
