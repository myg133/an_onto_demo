# 需求：REQ-171 · 让「概念 → 表/列」这条对齐通道真正打通

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-171 |
| 优先级 | **P1**（决定"文档 + 库"到底能不能对齐；REQ-170 只补了一半） |
| 状态 | 已就绪 |
| 基线 | `develop` `5a72429`（v0.3.0 + REQ-170） |
| 触发 | 用户提供真实 UAT 库 + 文档站做验证，实测 `maps_to` 落地 **0**；三次对照（A/B/C）定位到根因 |

## 实测证据（三次真跑，同一份文档、同样 4 块、真 ACP `pi-acp`）

| 跑 | schema 给了什么 | 断言 | 关系（活） | `maps_to` 落地 |
| --- | --- | --- | --- | --- |
| A | 全库 96 张（上限 40） | 75 | 8 | **0** |
| B | 按列注释挑出 14 张（实际带 8） | 97 | 14 | **0** |
| C | 同 B，**协议里手工加了 `maps_to` 字段** | 76 | 10 | **0** |

⇒ **排除**「选表不对」（B 给了语义相关表）与「协议缺字段」（C 加了仍 0）。

## 三个问题（按严重度）

### P1 · 概念→表这条通道**根本没开**

| 事实 | 证据 |
| --- | --- |
| `RelationAssertion` **有** `maps_to`（REQ-170 加，语义是"这条边由哪两列实现"= join） | `src/extract/doc_ingest.rs` |
| **`ConceptAssertion` 没有 `maps_to`** | 结构体只有 name/aliases/description/modality/alignable/evidence |
| 但 `doc_merge` **要求概念带 `maps_to.table`** | `doc_merge.rs:904`「concept「{}」的 maps_to 没有 table」 |

⇒ 合并端在等"概念→表"，抽取端连字段都没有 ⇒ 概念只能靠**名字/别名精确命中**对齐 ⇒ 这就是「与真实表对齐 3/755」一直没消掉的原因。
文档里的关系多是**抽象概念之间**（"旗舰模型 belongs_to Anspire token hub"），没有 join 列，REQ-170 的 `maps_to` 在这里用不上。

### P2 · 谓词闸与契约**打架**

| 规则 | 内容 |
| --- | --- |
| 契约（`Deploy/skills/ontok-extract/SKILL.md` §5.2） | 谓词只能从 **16 个主键**里选，其中**包含** `belongs_to` / `references` |
| 抽取闸（`doc_ingest.rs::STRUCTURAL_PREDICATES`） | 把 `belongs_to` / `references` 当"结构词"**直接丢弃**，提示「谓词不合规（空 / 结构词）」 |

实测每轮丢 11~13 条（**比活下来的还多**）：

| 跑 | 原始 | 活 | 丢 |
| --- | --- | --- | --- |
| A | 19 | 8 | 11（belongs_to 3 · references 8） |
| B | 27 | 14 | 13（belongs_to 1 · references 12） |
| C | 21 | 10 | 11（belongs_to 4 · references 7） |

### P3 · 选表打分在真实中文语料上几乎全 0

| 事实 | 值 |
| --- | --- |
| 表 | 96 |
| 表注释非空 | **25**（`customers`/`balances`/`api_call_logs` 都没有） |
| 打分 **> 0** 的表 | **1 / 96**（最高分 2） |
| 实际选中 top-12 | `order` · **`_volc_dts_pg_ddl_*` ×3（复制产物）** · `admin_roles`… |

而**紧凑全表清单（96 张：名+主键+一行注释）只要 3,221 字符** < 现有预算 12,000。
⇒ 打分全 0 时 top-N 退化成字典序；**小库上这个机制没必要且有害**。

## 要做四件

### R1 · `ConceptAssertion.maps_to`（核心）

```json
{"name": "云硬盘", "quote": "…",
 "maps_to": {"table": "cbs_disks", "column": "disk_id"}}
```

| 要求 | 细节 |
| --- | --- |
| 形状 | `{table, column?}`；`column` 可选（概念是整行时不给列） |
| 校验 | `table` 必须在 `--schema` 里；给了 `column` 则列必须在**该表**里 |
| 丢弃码 | 复用 REQ-170 的四类语义，但**概念专属**：`shape` / `table_unknown` / `table_ambiguous` / `column_absent`；计数进 `stats.maps_to.concepts.*` |
| 无 `--schema` | ⛔ 不许出现 `maps_to`（与关系同一条纪律） |
| 旧产物 | 读取端要能容忍省略（`Option`） |

### R2 · 运行时指令（`doc_acp.rs::OUTPUT_PROTOCOL`）+ 摘要措辞

1. 协议的 JSON 形状里**同时**给出两种 `maps_to`（概念→表；关系→两列），并说明**什么时候给**
2. 摘要里**明确要求**：「逐个概念尽量标注它落在哪张表；给不出就不写，⛔ 不许猜」
3. 摘要里说明「概念→表」与「关系→列」是两件事（现在的措辞只提了后者）

### R3 · 解谓词闸冲突（**放行** `belongs_to` / `references`）

判定：这两个词在契约的 16 个主键里，**是合法谓词**；把它们当"结构词"丢弃是**两套规则打架**。
要求：
- 从 `STRUCTURAL_PREDICATES` 里移除 `belongs_to` / `references`（保留 `has_many` / `related_to` / `is_a` 这类真正无信息量的）
- 同步更新契约（若契约里对它们的措辞需要改）与 `belongs_to_share` 计数口径
- ⛔ 不许让 AC-004 的判据失效：`belongs_to_share` 仍要能算（现在它恒 0，因为被闸丢了）

### R4 · 选表策略（小库给全表）

| 要求 | 细节 |
| --- | --- |
| 阈值 | 表数 ≤ `--schema-full-list-max`（默认 **200**）⇒ **直接给全表清单**（名 + 主键 + 一行注释），不做 top-N |
| 大库 | 仍走打分，但打分**主要看列注释**（中文的唯一桥），表名次要 |
| 垃圾表 | 排除：`_*` 前缀、`alembic_version`、以及"复制/DTS/迁移"类命名；排除要**可配**、并在屏幕报「排除了 N 张」 |
| 屏幕 | 报「给全表 96 / 或按打分带 40（共 96）」，让人知道走的是哪条路 |

## 判据（必须能红）

| # | 判据 |
| --- | --- |
| 1 | 有 `--schema` 且概念能对上表 ⇒ JSONL 里**概念带 `maps_to.table`**；`doc_merge` 能把它并进提案（不再走"名字精确命中"唯一的路径） |
| 2 | 概念 `maps_to` 的四类错误各有一条真进程反例 ⇒ 整条概念丢弃 + 计数 +1 |
| 3 | 无 `--schema` ⇒ 产物**逐字节不变**（含概念无 `maps_to`） |
| 4 | `belongs_to` / `references` **不再被丢弃**：同一份文档，改动前后这两个谓词的关系数**从 0 变成 >0**；`belongs_to_share` 不再恒 0 |
| 5 | 96 表的 schema ⇒ 走**全表清单**（屏幕报「全表」），且 `_volc_dts_pg_ddl_*` 类垃圾表**不出现** |
| 6 | 回归：`extract::`(259+) · `test_req170_schema_chain`(8) · `req163_local_chain`(8) · `req147_doc_acp`(13) · `req159_bundle`(38) · `req107_source_enumeration` · `anchor_guard` |

## 真实素材（可直接用，别重复采集）

| 文件 | 内容 |
| --- | --- |
| `.local/uat-corpus/schema-uat.json` | UAT 库真实 schema（96 表 / 1116 列 / 967 列有中文注释 / **0 外键**） |
| `.local/uat-corpus/schema-uat-subset.json` | 按列注释挑的 14 张子集 |
| `.local/uat-corpus/tokenhub-docs.clean.md` | 文档站语料（56k 字符） |
| `.local/uat-run2/{A,B,C}.jsonl` + `*.stats.json` | **三次真跑的产物**（对照用；C 是"协议加了 maps_to"的失败实验） |

⚠️ 真库 `anspire_open_uat`（`172.16.120.20:30432`）**只看不改**；要再跑真库必须保持只读。

## 范围

✅ 可改：`src/extract/**`、`src/cli.rs`、`src/review_open/build_run.rs`（如参数传递需要）、`src/domain/**`（如需）、测试。
⛔ 不动：`src/query/**`、`src/server/**`、`src/mcp/**`、`src/studio/**`、`src/db/**`、`src/provision/**`、`src/console/**`。
⚠️ `Deploy/skills/ontok-extract/SKILL.md` 由 **BA** 改（skills 在 deploy 分支）。
