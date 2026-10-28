# 状态：REQ-171 · 概念→表对齐通道

## 当前

**已完成**（合并 `9e48609`；F-1 文案补丁 `a278aec`）

## 我复核过的（lion 亲跑，带真库 `req171_lion`）

| 判据 | 结果 |
| --- | --- |
| `cargo check --lib` | 绿 |
| `--test test_req171_concept_maps` | **6 / 6** |
| `--lib extract::` | **270 / 270** |
| `--test test_req170_schema_chain` | 8 / 8 |
| **`--test test_req152_bins`（服务面守卫）** | **5 / 5** —— 服务端二进制仍不含构建面痕迹 |

## Dev 的关键实测（真 pi-acp + 真 UAT schema + 真语料，4 块）

| 判据 | 改动前（BA 的 A/B/C） | 本次 |
| --- | --- | --- |
| 概念 `maps_to` 落地 | 0 / 0 / 0 | **11 条**（丢弃 0） |
| `belongs_to` / `references` | 每轮丢 11~13 条 | **0 条被丢**（18 条全活） |
| `belongs_to_share.alive` | 0%（恒 0） | **3/26 = 11.5%** |
| 选表 | top-N 退化字典序（选中复制产物） | **全表 92 / 共 96**，跳过垃圾表 4 |
| `doc merge` | 概念只能靠名字精确命中 | **并入 11 条**（7 实体 `origin: both`） |

落地的 11 条全在真表：旗舰模型/模型目录/嵌入模型→`llm_model`、账号→`customers`、限流→`customer_rate_limit`、调用记录→`api_call_logs`、Token 计量→`usage_details`、账户余额→`balances`、点数→`free_amounts`。

## 根因（实验 C 的答案）

摘要末尾那句 **「概念断言**不写** `maps_to`。」** —— 协议里加了字段也没用，因为摘要明令禁止。
⇒ **协议 + 摘要 + 解析器三处必须同时说同一件事**。

## 我裁定的 8 条偏离

| # | 偏离 | 裁定 |
| --- | --- | --- |
| D-1 | 全表模式仍给「命中表」带列明细（92 张 + 40 张明细 = 10 512 字符 < 12 000） | ✅ **批准**（否则 REQ-170 的 `child_column` 没有依据） |
| D-2 | 屏幕报「全表 92 / 共 96 + 跳过 4」而非「全表 96」 | ✅ **批准**（更准） |
| D-3 | `--schema-full-list-max 0` = 永不走全表（便于对照） | ✅ 批准 |
| D-4 | 新增 `--schema-exclude-tables` / `--schema-keep-junk` | ✅ 批准（"排除要可配"） |
| D-5 | **内置通道 `SYSTEM_PROMPT` 未改**，仍写「不许写 belongs_to/references」 | ⚠️ **本单不改**（改了会破"无 schema 逐字节不变"判据，且内置通道是 legacy）→ **登记为已知不一致**：不带 `--agent acp` 时这两个谓词仍会被丢 |
| D-6 | `doc_merge` 不消费概念的 `column`（只留抽取产物） | ✅ 批准；记「`column` 目前仅作提示，merge 只用 `table`」 |
| D-7 | `has_one`/`has_many_to_many`/`is_a` 仍在闸上；变体随主词放行 | ✅ 批准 |
| D-8 | 概念表解析接受「中文名当表名」（唯一对上才收） | ✅ 批准 |

