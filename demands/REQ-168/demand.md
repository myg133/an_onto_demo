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


---

## 六 · 追加（2026-10-27 · 用户实测暴露的阻断）

**现象（用户贴的真实回执）**：agent 在客户环境里试了三种"它以为的用法"全失败，于是**没有使用脚本**：

```
which ontok-builder        ⇒ 找不到（构建工具没装在客户环境）
python -c "import ontok"   ⇒ ModuleNotFoundError（脚本不是 pip 包）
ls fixtures                ⇒ No such file（不在 agent 的 cwd）
```

**根因（两个叠加）**：
1. `SKILL.md` 写「脚本在同目录」+ 示例全是裸 `python access_fetch.py` ⇒ **假设 cwd = skill 目录**
2. `--selftest` 按 **cwd** 找 `fixtures/`（实证：单独复制 `.py` 到空目录 ⇒ `17 passed, 2 failed`）

**要做的**：
| # | 改动 |
| --- | --- |
| A | `FIXTURES_DIR = Path(__file__).resolve().parent / "fixtures"`；凡读内置夹具都走它（⛔ 不靠 cwd） |
| B | `fixtures/` 缺失时报**清楚的人话**（"请复制整个 ontok-query 目录"），⛔ 不崩栈 |
| C | `SKILL.md` 补「要跑 selftest 就复制整个目录；只取数则 `--mapping` 指向客户 AM」 |

**追加验收**：
| # | 判据 |
| --- | --- |
| AC-006 | 把 `access_fetch.py` **单独**复制到空目录 ⇒ `--selftest` 仍 **86 passed / 0 failed** |
| AC-007 | 删掉 `fixtures/` ⇒ 报人话错误并给修法，⛔ 不是 traceback |
| AC-008 | 从**任意 cwd**（含 `D:\` 根）用**全路径**跑 `--selftest` / `--check` 均正常 |


---

## 七 · 改判（2026-10-27 · 我的诊断被实测推翻）

### 我错在哪

| 我的诊断 | 实测事实 |
| --- | --- |
| 「`--selftest` 按 **cwd** 找 `fixtures/`」 | ❌ 它一直是 `dirname(abspath(__file__))/fixtures`。我引用的报错路径 `<复制后目录>\fixtures\…` **本身就是脚本位置**，被我误读成 cwd |
| 「只复制 `.py` ⇒ 仍要 86 绿」 | ❌ **不可满足**：88 条断言要读 7 个夹具，只复制 `.py` 时它们不存在 |

**反事实检验（我漏掉的那一步）**：cwd 换到无关目录、脚本留在原处 ⇒ 实测 **86 passed / 0 failed**。
（Dev 做的 T1。）⇒ 「cwd 影响自检」这条**不成立**。

### 改判后的验收

| 原 | 改为 |
| --- | --- |
| AC-006：只复制 `.py` ⇒ 86 绿 | **AC-006′：复制整个 `ontok-query/` 目录 + cwd 换到无关目录 ⇒ `--selftest` 88 passed / 0 failed，exit 0** |
| AC-007 | 不变：`fixtures/` 缺失 ⇒ 一行可操作报错（"复制整个目录"），**无栈、exit 1** |

**⛔ 否决两条捷径**：
1. 把夹具内嵌进 `.py`（+55KB，且治不了真实 AM 夹具）
2. fixtures 缺失时**静默跳过**签名/分页断言 —— 那是**把装坏的 skill 判成绿**，与 `load_auth_vector` 的"⛔ 不静默跳过签名验证"直接冲突

### 我的教训（写进执行清单）

> **报因果前先跑反例。**「我看到一个失败」≠「我知道它为什么失败」。
> 今天同型错误第 4 次（破规矩 / 编 exe 锁因果 / 编 MODES.md 引用 / 这次编 fixtures 路径成因）。
