# 状态：REQ-168 · 脚本认 contract_version=2

**已就绪**（2026-10-27 · 待派 Dev）

## 一句话

真 AM 是 v2，脚本只认 v1 ⇒ **生产上不可用**。改成认 `(1, 2)`，v3+ 仍拒。

## 依据（实测）

- `am-tencent.json` = v1，auth 只有 `mode`+`credential_env`
- `am-tencent.v2.json` = v2，auth 额外有 `signed_headers`/`timestamp`/`scope`
- 其余键一致（14 entities / 8 sources）
