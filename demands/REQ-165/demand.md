# REQ-165 · 通用化：脚本只按 AM 契约驱动，skill 只讲逻辑与流程

- 状态：**已就绪**（P1）
- 提出：2026-10-27
- 用户原话：**「skill 和脚本要做成通用版。我们是产品态，脚本只针对 AM 契约去做，skill 也只做逻辑和流程，做规定任务跟业务本体相关的特定说明。」**
- 关联：REQ-164（脚本首版）、REQ-144（AM 契约）、AUTH-SPEC

---

## 一 · 一句话

**现在这套带着"腾讯 + 云资源"的味道**（脚本 77 行、两份文档 20–26 行含厂商/业务字面量）。
产品态要求：**脚本 = AM 契约的执行器**（换一家云、换一套本体，一行不改），**skill = 任务与流程**（不含任何业务本体名词）。

---

## 二 · 现状（实测计数）

| 文件 | 含厂商/业务字面量的行 |
| --- | ---: |
| `access_fetch.py` | **77** |
| `SKILL.md` | **26** |
| `AM-FIELDS.md` | **20** |

具体形态：

| 位置 | 例子 | 问题 |
| --- | --- | --- |
| `access_fetch.py:378-380` | `if mode != "tc3-hmac-sha256"` ⇒ 直接报"只实现这个" | **单一签名实现写死在主流程里**，加第二种 auth 要改核心 |
| `access_fetch.py:85-91` | `TC3_DEFAULT_*` 常量 | 默认值是**某个 signer 的**，不该是全局 |
| `access_fetch.py` 的 `service` 推导 | 取 host 首段 | **猜**（换一家云可能不对） |
| `access_fetch.py:243,308` | 错误消息里写 `$.Response.InstanceSet[*]`、`am-tencent.json` | 把样例当成了规格 |
| `SKILL.md` / `AM-FIELDS.md` | `CvmInstance`、`cvm_instance`、`云服务器`、`子网`、`私有网络`、`Region` | **业务本体名词进了通用 skill** |

---

## 三 · 要做的

### ① 脚本：按 AM 契约驱动（核心改动）

| # | 改动 | 判据 |
| --- | --- | --- |
| 1 | **auth 变成签名器注册表** `SIGNERS = {"<mode>": Signer}`，主流程只做 `SIGNERS[auth.mode]` 查找 | 加一个假 vendor 的 signer ⇒ **不改主流程**就能跑（用一个 20 行的假 signer 证明） |
| 2 | **未知 `auth.mode`** ⇒ 新错误码 **`auth.unsupported`**（退出码 4），并**列出已支持的 mode** | 造 `auth.mode="hmac-sha1"` ⇒ 该码 + 列表，⛔ 不许 fallback 到 tc3 |
| 3 | 每个 signer 自带**它的默认值**（signed_headers / timestamp / scope / content_type / service） | 默认值不再出现在主流程；`tc3` 的默认值搬进 `Tc3Signer` |
| 4 | **`service` 必须来自 AM**（`auth.service`）；缺 ⇒ `am.invalid` 并**点名字段** | ⛔ 不再从 host 首段猜 |
| 5 | 错误消息/文档字符串里**不许出现具体样例**（`InstanceSet` / `am-tencent.json`） | 改成 `<JSONPath>` / `<am.json>` |
| 6 | 建议（可做则做）：`--list-signers` 打印支持的 mode | 输出形如 `tc3-hmac-sha256` |

### ② 文档：skill 只讲逻辑与流程

| 文件 | 要求 |
| --- | --- |
| `SKILL.md` §4.5 | **业务本体名词清零**：用 `<entity>` `<class>` `<source>` 占位；只讲**做什么、看什么、错怎么修**；具体某厂商的例子移到**附录「示例（某云厂商，仅供参考）」**并标明"示例不是规格" |
| `AM-FIELDS.md` | 逐键说明保留（那本来就是契约），但**举例用占位符**；`Region`/`CvmInstance` 这类换成 `<参数名>` `<entity id>` |
| `RECIPE.md` | ⛔ 不动（它本来*就是*业务特定那份） |

### ③ AM 契约补一条（**跨面**：AM 生成归另一位 BA）

`tc3-hmac-sha256` 模式下，**`auth.service` 必须显式声明**（不再靠猜）。
⇒ 需要：
- `AM-FIELDS.md` 标为 **tc3 必填**
- AM 生成器（`ontok-builder access`）输出该字段
- `am-tencent.json` 重新生成或手工补 8 个 source

⛔ **本单不代改 AM 文件**，只把"缺了会被拒"这条落到脚本与文档里。

---

## 四 · 验收

| # | 判据 |
| --- | --- |
| AC-001 | **禁止词扫描（脚本）**：`access_fetch.py` 里不出现 `CvmInstance` / `InstanceSet` / `cvm_instance` / `tencent` / `腾讯` / `云服务器` / `子网` / `私有网络` / `负载均衡` / `am-tencent`；**允许** `tc3-hmac-sha256` 与 `X-TC-*`（它们是该 signer 的规格） |
| AC-002 | **禁止词扫描（文档）**：`SKILL.md` 正文（不含附录示例）里同样清零；`AM-FIELDS.md` 举例全为占位符 |
| AC-003 | 假 vendor 的 signer（20 行）接入后能跑通 `--dry-run`：**主流程一行未改**（贴 diff 证明只加了注册项） |
| AC-004 | `auth.mode="hmac-sha1"` ⇒ `auth.unsupported` + **列出支持列表** + 退出码 4，⛔ 不许按 tc3 签 |
| AC-005 | AM 缺 `auth.service`（tc3 模式）⇒ `am.invalid` + 点名 `auth.service` |
| AC-006 | `--selftest` 仍全绿（**含签名向量**：tc3 的 Authorization 全串逐字节不变） |
| AC-007 | `--check` 对**现有** am-tencent.json：因缺 `auth.service` ⇒ **明确报错并指出怎么补**（这是预期的，因为 AM 还没补）；补上后 ⇒ 通过 |
| AC-008 | 既有 9 个错误码与退出码**一字不变**；新增的 `auth.unsupported` 排进码表（文档同步） |
| AC-009 | 把 `SKILL.md` §4.5（不含附录）单独抽出做关键词自检：**必须仍能描述该跑什么**（脚本名、6 个参数、9+1 个码、未签拒取、退出码 8） |

**红绿自证两条**：① 把 `service` 改回"host 首段"⇒ AC-005 红；② 给主流程加一个 `if mode == "tc3"` 分支（绕过注册表）⇒ AC-003 红。

---

## 五 · 不做

| 不做 | 为什么 |
| --- | --- |
| 真的实现第二种签名（AWS SigV4 等） | 本单只保证**架构上可插**，具体 signer 由需求驱动再加 |
| 改 `AUTH-SPEC.md` | 那是 tc3 的规格，仍有效 |
| 改 AM 文件本身 | 跨面（AM 生成归另一位 BA） |
| 把 `RECIPE.md` 通用化 | 它**本来就是**业务特定那份 |
