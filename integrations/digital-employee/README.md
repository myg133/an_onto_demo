# 数字员工平台 · ontok-query 适配层

> ⚠️ **这一层是"专用"的**：只对**本平台**成立。
> 产品 skill（`Deploy/skills/ontok-query/`）**不认识本平台** —— 它只认 AM 契约。

## 一 · 本平台的三个约定（产品不该知道）

| # | 约定 | 后果 |
| --- | --- | --- |
| 1 | **凭据注入名带前缀**：`SKILL_CREDENTIAL_<字段名>`（如 `SKILL_CREDENTIAL_SECRETID`） | AM 里写的 `TENCENT_SECRET_ID` **取不到值** ⇒ 产品脚本报 `credential.missing`（这是**正确**行为） |
| 2 | **沙箱没有 shell**：`run_skill_script` 直接执行脚本 | skill 里教的 `VAR=… python …` 桥接**执行不了** |
| 3 | **agent 只能"读回执 → 写文件 → 喂脚本"** | 它会把 `@pending` **手抄**成文件，容易丢字段（实测丢过 `pushdown`） |

## 二 · 因此在本平台要这么调

```bash
# 配方：把查本体拿到的回执**原样**存盘（脚本认四种形状，不用做手术）
# 凭据：把 AM 声明的名字显式指到本平台注入的名字（⛔ 值不落盘、不回显）
python scripts/access_fetch.py --recipe recipe.json --format rows --out ./rows.json \
  --credential-map TENCENT_SECRET_ID=SKILL_CREDENTIAL_SECRETID \
  --credential-map TENCENT_SECRET_KEY=SKILL_CREDENTIAL_SECRETKEY
```

先探一下注入了哪些名字（只打印名字与长度，⛔ 不打印值）：

```bash
python integrations/digital-employee/env_probe.py
#   NO_SKILL_CREDENTIAL_ENV ⇒ skill 的 credentials 声明 / 员工绑定 / 凭据状态 三处之一没生效
```

## 三 · 为什么不在产品 skill 里

| 放进产品 | 会发生什么 |
| --- | --- |
| `SKILL_CREDENTIAL_*` 写进产品 skill | 下一个人以为**产品认识这个平台**；换个平台又得改产品 |
| 产品脚本"自动猜"前缀 | 猜到了就是隐式耦合，猜不到就是"神秘失败"；两者都比**显式映射**差 |

⇒ **产品保证"AM 写什么就读什么"**；平台差异由本层用 `--credential-map` 显式表达。

## 四 · 平台侧还要配的两件事（不在本仓库）

1. skill 元数据里声明 `credentials:`（`id` 由平台分配）
2. 员工绑定该 skill，并确认凭据状态为"启用"
