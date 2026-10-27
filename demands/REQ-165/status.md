# 状态：REQ-165 · 通用化（脚本按 AM 契约驱动 / skill 只讲流程 + 讲契约）

## 当前状态

**已完成**（2026-10-27 · `Deploy` `f5e1145`，待推）

## 起因（用户原话）

> 「skill 和脚本要做成通用版。我们是产品态，脚本只针对 AM 契约去做，
>   skill 也只做逻辑和流程，做规定任务跟业务本体相关的特定说明。」
> 追加：「skill 里补充一下，还有讲契约。」

## 交付

| 项 | 结果 |
| --- | --- |
| `access_fetch.py` | 2166 → **2478 行**；`Signer` 基类 + `Tc3Signer` + `SIGNERS` 注册表；`auth.unsupported` 新码；`--list-signers` |
| `SKILL.md` | 604 → **736 行**；新增 **§4.6「AM 契约：数字员工可以依赖什么」**（八小节）；业务示例移入附录 B 并标「示例不是规格」 |
| `AM-FIELDS.md` | 245 → 256 行；`auth.service` 标 tc3 必填；举例全占位符 |
| 禁止词（AC 口径 10 词） | 脚本 **9 → 0** · SKILL 正文 **19 → 0** · AM-FIELDS **11 → 0**（BA 自己复跑：0/0/0） |
| `--selftest` | **86 passed / 0 failed**（前 71）；tc3 `Authorization` 全串逐字节不变 |
| 真 AM `--check` | **8 error / exit 2** —— `auth.service` 缺失，**fail-closed 如设计**（AM 补齐由 REQ-166 做） |

## ⚠️ 我自己的一处口径错误（必须记）

我在立单时写"脚本 77 行 / SKILL 26 行 / AM-FIELDS 20 行含厂商业务字面量"。
**那是更宽的统计口径**（把 `tc3` / `X-TC-*` / `Region` / `Response.` 也算进去了）。
按 AC-001 的 **10 个词**复算是 **9 / 19 / 11**。

⇒ 教训：**报"有多少处"必须同时报"按什么口径"**，否则交付方按 AC 口径复算不出来，会以为需求写错了。

## 三条裁定

| # | 事项 | 裁定 |
| --- | --- | --- |
| 1 | `--check` 在 `auth.mode` 不支持时退出码 **2 → 4**（新码导致） | **接受**。既有 9 个码退出码一字不变；`--check` 非零本就表示"AM 有问题"，硬判 `==2` 的调用方属于用错 |
| 2 | AC-001（脚本禁厂商名）与 AC-006（Authorization 逐字节不变，host 进规范串）冲突 | **接受 dev 的解法**：签名向量搬到 `fixtures/auth-vector.json`，脚本只留「读向量 → 取签名器 → 对拍」。**fixtures 是证据/示例区，不是逻辑区**；逻辑里 0 命中 |
| 3 | `paging_plan` 在 AM 没写 `offset_param`/`limit_param` 时兜底 `Offset`/`Limit` | ⛔ **不许兜底**（与"不许猜 service"同一类）⇒ 另开 **REQ-167** 收掉 |
| 4 | 业务错误信封只认 `<信封>.Error{Code,Message}`（换信封的源落到 `api.invalid`） | **契约缺口**：AM 应能声明错误信封 ⇒ 记入 REQ-167 的 AM 侧依赖（归 AM 生成方） |
