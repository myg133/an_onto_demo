---
name: ontok-build
description: FDE 用：把客户文档（和/或关系库）变成一套可发布的本体 —— 从摄取、合并、人签、冻结到导出 TTL 与发布。含每步的验证方法与「失败长相」对照表。不涉及消费侧调用（那见 ontok-query）。
category: workflow
---

# ontok-build · 用文档（和库）造一套本体

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

## 二 · 路径 A（有库）· 六步

```bash
# 1 勘查（只读，不搬数据）
ontok-introspect --out work/schema.json
#   ✅ 验：表数 / 列数 / 有注释的比例
#      空库或全无注释 ⇒ 先回去问客户"这套库是不是对的"

# 2 生成翻译表（表名中文名）—— **--names 现在是必填**
ontok name suggest --schema work/schema.json --out work/names.json --as-names work/table_names.json
#   ✅ 验：覆盖了多少张表（覆盖不全 ⇒ 后面实体名会是英文/占位）

# 3 提案（关系 + 命名）
ontok-propose --schema work/schema.json --names work/table_names.json \
              --out work/proposal.json --fk-out work/fk-proposals.json
#   ✅ 验：关系条数、strong / medium / 否决 各多少；样本命中率

# 4（可选）用文档补充语义
ontok doc ingest --input docs/ --out work/assertions.jsonl --stats work/doc-stats.json
ontok doc merge  --assertions work/assertions.jsonl --proposal work/proposal.json \
                 --schema work/schema.json --out work/merged.json --stats work/merge-stats.json
#   ✅ 验：并入 / 排除不可对齐 / 未映射 / 冲突 五项

# 5 人签（⛔ 不可自动化）
ontok project new --name <项目>
ontok version new --project <项目>
ontok propose import --file work/merged.json
ontok review decide --proposal p1 --project <项目> --accept-all   # 或 review serve 逐条
ontok proposal apply --proposal p1 --project <项目>
ontok version freeze --project <项目> --version 2

# 6 导出
ontok ontology export --project <项目> --version 2 --out out/customer.ttl
ontok ontology export --project <项目> --version 2 --model-view --schema work/schema.json \
                      --tenancy work/tenancy.json --out out/model.ttl
ontok r2rml generate --project <项目> --version 2 --schema work/schema.json --out out/mapping.ttl
#   ✅ 验：类 / 关系 / 属性 三个数；TTL sha256
```

---

## 三 · 路径 B（纯文档）· 五步

```bash
# 1 摄取（要 LLM；按批计 token）
ontok doc ingest --input docs/ --out work/assertions.jsonl \
                 --stats work/doc-stats.json --budget-tokens 500000
#   ✅ 验：断言数 / 丢弃（quote 找不到）条数 / 用了多少 token
#   ⚠️ 超预算 ⇒ 一条断言都不产出（设计如此，"不产出半份"）

# 2 合并成提案 —— **必须带 --allow-doc-only**（否则空提案会把全部断言判为未映射）
ontok doc merge --assertions work/assertions.jsonl --proposal work/empty-proposal.json \
                --allow-doc-only --out work/merged.json --stats work/merge-stats.json
#   ✅ 验：新建实体数 / 未映射 / 冲突
#   ⚠️ 空提案文件内容：{"version":"1.0.0","items":[]}

# 3 人签
ontok project new --name <项目> && ontok version new --project <项目>
ontok propose import --file work/merged.json
ontok review decide --proposal p1 --project <项目> --accept-all
ontok proposal apply --proposal p1 --project <项目>
ontok version freeze --project <项目> --version 2

# 4 导出（B 没有 schema ⇒ 不带 --schema）
ontok ontology export --project <项目> --version 2 --out out/customer.ttl

# 5 ✅ 验：类数 = 断言里的概念数（去重后）；关系数 = 两端都能解析的关系数
```

### ⚠️ 路径 B 的两个常识

**① 概念会很多**

原始断言是"原文片段级"的。728 条断言可能抽出 **506 个概念**——直接全建会得到一个没法用的本体。

**建议**：先用 `--budget-tokens` 控制范围，或**只喂核心文档**（规范/流程），别一开始就灌全部会议纪要。

**② 关系很脆**

文档里的关系是"动词"（"使用"、"绑定"、"降级"）。两端名字必须在概念集合里**精确对得上**——差一个字就进 `doc_unmapped`。

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
ontok ontology export --project <项目> --version <N> --out out/customer.ttl

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

```
□ 有库吗？→ A / B 选一条
□ 文档是 .md/.txt 且 UTF-8？
□ ONTOK_LLM_* 三件套在？
□ --names 显式给了真实翻译表？
□ 纯文档走了 --allow-doc-only？
□ 五项合并统计看过？（并入/排除/未映射/冲突/doc-only）
□ 人签是真的人签的？
□ 冻结后版本号记住了？
□ 导出 TTL 的 sha256 记了？
□ 发布后四条验证都过了？
```
