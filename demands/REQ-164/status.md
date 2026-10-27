# 状态：REQ-164 · AM 取数脚本进 skill

## 当前状态

**已就绪**（2026-10-27 · 已派 Dev）

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
