# 状态：REQ-165 · 通用化（脚本按 AM 契约驱动 / skill 只讲流程）

## 当前状态

**已就绪**（2026-10-27 · 待派 Dev）

## 起因（用户原话）

> 「skill 和脚本要做成通用版。我们是产品态，脚本只针对 AM 契约去做，
>   skill 也只做逻辑和流程，做规定任务跟业务本体相关的特定说明。」

## 现状（实测计数）

| 文件 | 含厂商/业务字面量 |
| --- | ---: |
| `access_fetch.py` | **77 行** |
| `SKILL.md` | **26 行** |
| `AM-FIELDS.md` | **20 行** |

## 三个核心改动

1. **auth 变签名器注册表**（`SIGNERS[mode]`），未知 mode ⇒ `auth.unsupported` + 列支持列表（⛔ 不 fallback）
2. **`service` 必须来自 AM**（`auth.service`），缺 ⇒ `am.invalid` 点名（⛔ 不从 host 猜）
3. **文档业务名词清零**，示例移到带「示例不是规格」标注的附录

## 跨面依赖（归另一位 BA）

`tc3` 模式要 `auth.service` 显式声明 ⇒ AM 生成器要输出它，`am-tencent.json` 要补 8 个 source。
⛔ 本单不代改 AM 文件。
