# 状态：REQ-164 · AM 取数脚本进 skill

## 当前状态

**已完成**（2026-10-27 · `deploy` `e4507d8` 已推）

## 起因（用户原话）

> 「AM 的查询脚本做到 skill 内。这样，数字员工拿到 AM 就不用去自己做查询脚本了，还总出错。」

## 交付物（都在 `Deploy/skills/ontok-query/`）

| 文件 | 内容 |
| --- | --- |
| `access_fetch.py` | Python 3 标准库，零第三方依赖 |
| `AM-FIELDS.md` | AM 每个键是什么、脚本用了哪些 |
| `SKILL.md` | 新章「取数用脚本，别手写」+ 常见错误表 |

## 关键决定

| # | 决定 | 理由 |
| --- | --- | --- |
| 1 | **Python 3 标准库**，不用 `requests` | 客户/agent 环境普遍有 Python；零依赖才不会"装不上" |
| 2 | **只从 `credential_env` 命名的环境变量读凭据** | 凭据不进文件/命令行/日志 |
| 3 | **未签 entity 不许取数**（`reviewed_by` 空 ⇒ 拒） | 与 R6 同一条纪律 |
| 4 | **取不全要如实说**（`meta.truncated` + 退出码 8） | 静默截断是最坏的错 |
| 5 | 参数只允许 `capability.pushdown` 里的 | 给了别的会被 API 拒，不如本地就拦住 |

## 交付结果

| 项 | 值 |
| --- | --- |
| commit | `Deploy` **`e4507d8`**（已推） |
| `access_fetch.py` | **2166 行**，Python 3.8+ 标准库，**13 个 import 全 stdlib** |
| `AM-FIELDS.md` | 245 行（AM 逐键 ✅/◐/⛔） |
| `SKILL.md` | 499 → 604 行，新增 §4.5 |
| BA 亲验 | `--check` 真实 AM 通过（8 sources / 14 entities，1 warning）· `--selftest` **71 断言全绿**（自检期 socket 被换成抛异常 ⇒ 断网可过的机器证明） |

## 遗留两条（AM 侧，归 AM 生成方）

1. `sources[].auth` 缺 `signed_headers`/`timestamp`/`scope`/`service` ⇒ 脚本按 TC3 默认签名（`service` 取 host 首段），`--check` 每次提示
2. `CdbInstance.instance_ids` 缺 `"shape": "array"` ⇒ `--param instance_ids=a,b` 会被当成一个 ID

## 一条**待用户**的（唯一可能真调翻车的点）

`Region` 写在 body 模板里（10 个 entity），脚本因此**不发 `X-TC-Region` 头**。
RECIPE §七① 说同值进 header+body 会 `UnknownParameter`；但**只有真凭据跑一次**才能定论。
⇒ 需要用户提供腾讯凭据（或接受风险并写进文档）。
