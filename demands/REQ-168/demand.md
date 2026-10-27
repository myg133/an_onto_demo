# REQ-168 · 取数脚本要认 `contract_version=2`

- 状态：**已就绪**（P1）
- 触发：另一位 BA 用真 AM 跑 `--check` ⇒ `[am.invalid] contract_version=2，本脚本只认 1`（exit 2）
- 关联：REQ-164（脚本首版）、REQ-165（通用化）、REQ-166（AM 补字段）

## 一 · 事实（实测两个文件）

| 文件 | `contract_version` | `auth` 内容 |
| --- | --- | --- |
| `.local/access-mapping/am-tencent.json` | **1** | `mode` + `credential_env` |
| `.local/am-v2/am-tencent.v2.json` | **2** | `mode` + `credential_env` + **`signed_headers` / `timestamp` / `scope`** |

`entities[]` 与 `sources[]` 的**其余键完全相同**（14 entities，14 个键；8 sources，5 个键）。
⇒ v1→v2 的差异，从**数据**看只有"auth 可以显式写出签名参数"这一点。

## 二 · 问题

`access_fetch.py:89` 硬编码 `CONTRACT_VERSION = 1`，`:363-364` 严格相等比较。
⇒ **真 AM（v2）直接被拒**，脚本在生产上不可用。

## 三 · 要做的

| # | 改动 |
| --- | --- |
| 1 | 改成 `SUPPORTED_CONTRACT_VERSIONS = (1, 2)`，逐版本给一行说明（v2 = auth 可显式声明签名参数） |
| 2 | 不认识的版本（如 3）⇒ **仍然拒**（`am.invalid`），并列出支持的版本 —— ⛔ 不许"尽力解析" |
| 3 | `--check` 输出里打印**识别到的版本**与"该版本下脚本读了哪些键" |
| 4 | 文档（`SKILL.md` §4.6 与 `AM-FIELDS.md`）写清 v1/v2 差异与脚本对两者的行为 |

## 四 · 验收

| # | 判据 |
| --- | --- |
| AC-001 | 对 `am-tencent.v2.json` ⇒ `--check` 通过（除 `auth.service` 缺失外无其它错） |
| AC-002 | 对 `am-tencent.json`（v1）⇒ 行为与现在**完全一致**（不回归） |
| AC-003 | `contract_version=3` ⇒ `am.invalid` + 列支持版本（`1, 2`）+ exit 2 |
| AC-004 | `--selftest` 全绿，且新增"v2 可读 / v3 被拒"两条 |
| AC-005 | 若 v2 的 auth 显式给了 `signed_headers`/`timestamp`/`scope`，**签名结果必须与"用默认值"逐字节相同**（同一向量对拍） |

## 五 · 待确认（已去问 AM 侧）

- v2 的**正式变更清单**（是不是只有 auth 一项）—— 若还有别的，按清单补
- 真 AM 里 `auth.service` 到底补没补（我在 `.local/am-v2/am-tencent.v2.json` 里**没看到** `service`）
