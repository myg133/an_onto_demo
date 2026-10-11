# REQ-008 独立复核报告

## 0. 审核元信息

| 项 | 内容 |
| --- | --- |
| 审核范围 | `feature/REQ-008` @ `300e2cd`（3 提交：`71d7029` / `14a7049` / `300e2cd`），基线 `develop` @ `d7120e4` |
| 审核工作区 | `.review-workspace/audit-r8`（分支 `audit/req008`，**未改动** `code/`、`feature-REQ-008/`、`BA/`） |
| 环境 | cargo 1.98.1；本体库可达（`172.16.120.20:30432` 端口通）；口令只走环境变量，未 echo、未落盘 |
| 变异目标 | 8 个变异全部重跑，日志 `r8-artifacts/indep-mutate-log.txt` |
| 关键方法 | 不信自述：直接调真实代码（临时探针，已从 `tests/` 移出，git 树干净）+ 全量 `cargo test` + 重生成页面逐字节比对 |

### 一句话结论

**判读材料不能按现状放行 —— 代码修复本身是扎实的（7 条自述 7 条成立），但页面上有 3 处会让 3 名外部评审人答错 Q1/Q2 的问题，且都不在 Dev/BA 的自述范围内。**

### 7 条自述裁定表

| # | 自述 | 裁定 | 独立证据 |
| --- | --- | --- | --- |
| 1 | 客户标识 10→0、客户 6→0、用户 5→0；所属客户 12→**33** | ✅ **成立** | 我用脱敏 `schema.json` 独立复算：全页 `客户标识`/`客户ID`/`客户`/`用户ID`/`用户id`/`用户 ID` = **0**，`所属客户` = **33**（页面与提案两处都是 33） |
| 2 | 主客户 1→1、子账号 4→4、划拨来源 1→1 逐字未变 | ✅ **成立** | 18 条角色限定列逐列核对：7 类模式（main/sub/source/from/to/target/related）**全部 `infer_link=None`**，注释原样；页面 `主客户`=1 |
| 3 | 8 个变异全过，含 1 个阴性对照 | ✅ **成立（但自述的「证据」不成立，见 §4）** | 我独立重跑：M1–M7 **RED**、M8 **GREEN**、8 次还原 sha256 逐字节一致、收尾 `git diff --quiet HEAD` 干净 |
| 4 | 屈折：词表 207→207 一条没加，丢弃率 8.4%→7.8%，救回 6 条 | ✅ **成立** | 4 张词表与 develop **内容完全相同**（11/207/21/5）；实测丢弃 75/964=**7.8%**；救回 6 条全部在页面上可见 |
| 5 | `name,email` 判 LowQuality，英文不进客户视图 | ✅ **成立** | 端到端探针：`c_name_email` → `属性名=""`、`说明=None`，**结构上无法进客户视图** |
| 6 | S-2 红线未动：`test_humanize_still_refuses_unknown_columns` 一断言没改 | ✅ **成立** | 该测试在 develop↔分支 diff 中 **0 行**。全仓测试仅 1 条既有断言被改（`扣费流水: license 标识`→`所属授权`），已在测试注释里说明并由新测试正面钉住 |
| 7 | 215 全绿零回归 | ✅ **成立** | 我自己跑：`215 passed; 0 failed`（57+1+18+4+13+6+16+4+10+8+9+5+28+10+26） |

> 7/7 成立。但**成立的是「代码改对了」，不等于「材料够格」**——见 §5。

### 本次完成的任务

| 任务 | 状态 |
| --- | --- |
| P0-1 外键误统一 | ✅ 完成（含 163 列全量模拟 + 真实代码抽取复算） |
| P0-2 屈折真伪 | ✅ 完成（新词根/多层/边界/文档-代码一致性） |
| P0-3 字段名列表边界 | ✅ 完成（11 组输入 + 端到端 + 原子追溯） |
| P0-4 页面够不够格 | ✅ 完成（含重生成逐字节比对 + 自建独立词表扫描） |
| §4 变异自证复核 | ✅ 完成（8/8 独立重跑） |
| P1 | 未做（时间预算）；未做的事已在 §6「我没能验证的部分」列明 |

---

## 1. 任务 1 · 外键统一有没有误伤

### 1.1 逐列核对结果

全库 `*_id` / `*_ids` / `*_no` / `*_code` 共 **163** 列。按 `src/extract/propose.rs:707` `infer_link` 的真实算法复算：
**52** 条能推出关系，其中 `customers.customer_no` 被 `propose.rs:290` 的 `.filter(|t| t != &table_name)` 自引用守卫滤掉 → 实得 **51** 条（与 `links_inferred: 51` 吻合）。

再对 51 条跑真实 `extract()`，结果分三组：

**A) 改成「所属X」= 42 条，全部语义正确**

| 目标 | 条数 | 显示名 | 判定 |
| --- | --- | --- | --- |
| `customers` | 33 | 所属客户 | ✅ 正确 |
| `admin_users` | 3 | 所属后台管理员 | ✅ 正确 |
| `licenses` | 3 | 所属授权 | ✅ 正确（含 `license 标识`→英文名复述，`300e2cd` 修的正是这个） |
| `order` | 2 | 所属订单 | ✅ 正确 |
| `activity` | 1 | 所属活动 | ✅ 正确 |

**B) 保留原注释 = 9 条，全部该保留**

| 列 | 保留下来 | 判定 |
| --- | --- | --- |
| `app_billing_transactions.customer_id` | 被代扣Open用户ID | ✅ **正是 briefing 点名的那条，没被误统一**（含拉丁 `Open` → 剥不出纯对象名 → 保守保留） |
| `balance_operation_log.customer_id` | 账号ID（余额所属账号） | ✅ 括号限定 |
| `dim_customer_binding.customer_id` | 账号ID（统计维度账号） | ✅ 括号限定 |
| `frozen_assets.customer_id` | 子账号ID | ✅ `子账号` 不是对象名/词表值 → 保留 |
| `customer_interests.order_id` | 系统订单序列号id | ✅ |
| `free_amounts.share_usage_id` | 关联的分享使用记录ID | ✅ |
| `proxy_operation_auths.admin_user_id` | 授权管理员ID | ✅ |
| `proxy_operation_records.admin_user_id` | 操作管理员ID | ✅ |
| `api_call_logs.license_id` | （LowQuality→词表兜底） | ✅ `License ID` 没借外键身份绕过门槛 |

**C) 该改没改 = 0 条**（51 条里没有一条「能推出关系却没改名」）。

**结论：51 条可解析外键，0 误统一。** 判据偏保守的设计（`propose.rs:619` 起的 `fk_comment_is_restatement` 只认「剥完正好是一个对象名」）在真实数据上表现为**零信息损失**。

### 1.1b briefing 点名的 7 类角色限定列 —— 逐列实况

| 列 | 表 | 原注释 | `infer_link` | 提案名 |
| --- | --- | --- | --- | --- |
| `main_customer_id` | customer_account_binding / deductions / sub_account_quota | 主账号ID | None | 主账号ID |
| `main_customer_id` | dim_customer_binding | 主账号ID（独立账号时为NULL） | None | 原样 |
| `main_customer_id` | frozen_assets | 主账号ID（冻结操作者） | None | 原样 |
| `main_customer_id` | sub_account_bind_permission | 绑定的主账号ID | None | 原样 |
| `main_customer_id` | family_high_risk_records | （无注释） | None | 空→词表 `主客户` |
| `sub_customer_id` | 4 张表 | 子账号ID | None | 子账号ID |
| `source_customer_id` | balances | 划拨来源主账号ID | None | 原样 |
| `from_customer_id` | balance_transfer | 转出账号（主账号） | None | 原样 |
| `to_customer_id` | balance_transfer | 转入账号（子账号） | None | 原样 |
| `target_customer_id` | simulate_login_sessions | 被模拟用户ID | None | 原样 |
| `related_customer_id` | balance_operation_log | 关联账号ID | None | 原样 |

**Dev 的说法成立，且比他自己说的更彻底**：不只是 7 类，还有 `related_customer_id` / `binding_id` / `share_usage_id` / `original_record_id` 等带角色的列，共 **73 条**未解析的 `*_id` 带中文注释列**逐条保留**。

### 1.2 页面上所有非「所属X」形式的外键属性名

页面渲染时 `src/domain/html_view.rs:802` 会剥掉尾部 `ID`（既有 REQ-007 逻辑，非本轮）。页面上非「所属X」的外键属性名共 4 类，**全部语义正确**：

| 页面显示 | 出自 | 次数 |
| --- | --- | --- |
| 主账号 / 主账号（冻结操作者）/ 主账号（独立账号时为NULL）/ 绑定的主账号 | `main_customer_id` 6 处 + 注释变体 | ~7 |
| 子账号 | `sub_customer_id` 4 + `frozen_assets.customer_id` 1 | 5 |
| 划拨来源主账号 | `source_customer_id` | 1 |
| 转出账号（主账号）/ 转入账号（子账号） | `from_/to_customer_id` | 1+1 |
| 被模拟用户 / 关联账号 / 账号（余额所属账号）/ 账号（统计维度账号） | 限定型 `customer_id` | 各 1 |
| 系统订单序列号 / 关联的分享使用记录 | 限定型 | 各 1 |

**命名不再统一，但每一处都更准确。** 评审人看到「主账号/子账号」比看到统一成「所属客户」更能对上行（`balance_transfer` 的转出/转入方向、划拨来源都保住了）。这一项**不构成缺陷**。

---

## 2. 任务 3a · 屈折是真机制还是堆词

### 2.1 词表确实一条没加

`git show develop:src/domain/view.rs` 与分支版逐条比对：

| 词表 | develop | 分支 | 内容完全相同 |
| --- | --- | --- | --- |
| `COLUMN_LABELS` | 11 | 11 | ✅ True |
| `BUSINESS_WORDS` | 207 | 207 | ✅ True |
| `FORM_SUFFIXES` | 21 | 21 | ✅ True |
| `TECH_SUFFIXES` | 5 | 5 | ✅ True |

且 `prices` / `models` / `cycles` / `weekdays` / `notices` / `categories` / `available_cycles` / `tokenhub_notices` **一个都不在 `BUSINESS_WORDS` 里**。**自述 #4 的「一条没加」核实成立。**

### 2.2 是机制不是枚举 —— 直接构造新词根验证

| 输入 | 结果 | 期望 | 判定 |
| --- | --- | --- | --- |
| `zzz_cycles` | `None` | None | ✅ 认后缀不认词根 → 失败 |
| `qqq_prices` | `None` | None | ✅ |
| `xxx_weekdays` | `None` | None | ✅ |
| `zzz_models` / `zzz_notices` / `zzz_addresses` | `None` | None | ✅ |
| `yyy_categories` | `None` | None | ✅ |

**凭空翻不出来 → 确实是规则。**

### 2.3 收益：正好 6 条，与自述一致

| 列 | 页面显示 |
| --- | --- |
| 套餐产品/`models` | 模型 |
| 套餐模板/`prices` | 价格 |
| 套餐模板/`available_cycles` | 可用周期 |
| 服务高峰配置/`weekdays` | 工作日 |
| 网站公告配置/`tokenhub_notices` | 令牌中心公告 |
| 网站公告配置/`tokenhub_notice_versions` | 令牌中心公告版本 |

实测丢弃率 **75/964 = 7.8%**（`tests/drop_rate.rs` 亲跑）。6 条救回 ⇒ develop 上为 81/964 = 8.4%。**8.4%→7.8% 核实成立。**

### 2.4 边界：结果对，但有一处「文档说有、代码没有」

| 输入 | 结果 | 判定 |
| --- | --- | --- |
| `address` / `addresses` | 地址 / 地址 | ✅ `ss` 守卫生效（`view.rs:444`） |
| `status` / `statuses` | 状态 / 状态 | ✅ `us` 守卫生效 |
| `analysis` | `None` | ⚠️ **结果对，机制不对** |
| `gas` / `bus` / `is` | `None` | ✅ 长度守卫（`tok.len() > 3`） |

**发现（低危 · 文档与代码不符）**：`src/domain/view.rs:400-402` 的文档注释写「以 `us` / **`is`** 结尾（`status` / `analysis`）…不碰」，但 `view.rs:444` 的代码只有 `!ends_with("ss")` 和 `!ends_with("us")` 两个守卫，**没有 `is` 守卫**。`analysis → analysi` 之所以是 `None`，是因为 `analysi` 不在词表里 —— **碰巧对，不是按规则挡的**。`tests/test_prop_name_quality.rs::test_inflection_is_morphology_not_wordlist` 断言的是**结果**不是机制，所以照样绿。

- 触发条件：将来往 `BUSINESS_WORDS` 加一条形如 `analysi` / `basi` / `thesi` 的键。
- 当前影响：**无**（fail-closed，词表未命中仍 `None`）。
- 建议：要么补 `!ends_with("is")`，要么把文档里那句 `-is` 删掉。**不阻断判读。**

### 2.5 多层复合：只支持一层（已文档化 + 已测试，属限制非缺陷）

`a_b_cycles` / `original_prices` / `a_b_c_prices` / `peak_available_cycles` **全部 `None`**。`view.rs:452` 的 `inflected_column_forms` 只动最后一个词元，文档明说这是有意的；`original_prices` 甚至被测试正面钉成 `None`。

- 真实代价：`套餐模板/original_prices`、`套餐模板/cooling_hours` 至今仍在丢弃名单里。
- 判定：**这是取舍不是 bug**，不阻断。下一轮若要救这类，需要多词元递归（当前 `resolve_stem` 不支持）。

---

## 3. 任务 3b · 字段名列表判据的边界

### 3.1 目标用例已封死（自述 #5 成立）

| 输入 | 判定 | 端到端结果 |
| --- | --- | --- |
| `name,email` | LowQuality | `属性名=""`、`说明=None` |
| `name,email,phone` / `name, phone` | LowQuality | 同上 |
| `name/email` / `name;email` / `name|email` | LowQuality | 同上 |
| `type,status` / `name,name` | LowQuality | 同上 |

### 3.2 真值域仍然全认（**除 1 处例外，见 3.4**）

`processing/charged/failed` · `0: llm, 1: emb` · `active/inactive` · `Y/N` · `M/D/H` · `utf-8/mb4` · `a;b;c` · `ok,failed,timeout` · `credit/recharge/free` → **全部 EnumDomain** ✅

Dev 的说法「`llm` 就在词表里，但 `0: llm` 有编码前缀所以不吃掉」**核实成立**：`propose.rs:512` `has_code_prefix` 使 `is_field_name_list` 直接返回 false。

### 3.3 残留 fail-open：比 Dev 记录的更宽

| 输入 | 判定 | 后果 | Dev 文档里记了吗 |
| --- | --- | --- | --- |
| `0: name, 1: email` | EnumDomain | **英文字段名进「说明」**（端到端实测 `说明=Some("0: name, 1: email")`） | ❌ **没记**，只记了 `a/b/c` |
| `code,name` / `id,code` / `url,name` | EnumDomain | 同上泄露 | ❌ 没记 |
| `a,b` / `a/b/c` / `gas,bus` | EnumDomain | 同上泄露 | ✅ 记了（但只提「单字母+三项以上」，实际两项也中招） |
| `名称,邮箱` | HumanReadable | **字段名列表当成了属性名**（`属性名="名称,邮箱"`） | ❌ 没记 |

- 根因一：`is_field_name_list` 的「全有或全无」用 `is_known_column_word` 判定，而 `code` / `url` **不在** `COLUMN_LABELS`/`BUSINESS_WORDS`（实测 `is_known_column_word("code")=false`），于是含它们的列表整体漏过。
- 根因二：`has_code_prefix` 把「带编码前缀」直接等同于「是值」，但「带编码前缀的字段名列表」也是真实存在的形态。
- **对本页无实际影响**：真实源库只踩到 2 条 ASCII 值域（`0: llm, 1: emb`、`processing/charged/failed`），两条都是**合法值域、故意保留**。这是**机制有洞、数据没踩**，与 Dev 的判断一致。
- 判定：**建议项，不阻断**。但 Dev 文档里的「已知的剩余漏洞」清单不完整，建议补上。

### 3.4 ⚠️ 本轮引入的一处行为回退（中等 · fail-closed 方向）

| 输入 | develop | 分支 | 变化 |
| --- | --- | --- | --- |
| `POINT/RMB/USD` | **EnumDomain**（值域保留） | **LowQuality** | 值域信息**被删干净** |
| `RMB/USD` / `point/rmb/usd` / `POINT/RMB` | EnumDomain | LowQuality | 同上 |

- 根因：`is_known_column_word("point"/"rmb"/"usd")` **全为 true**（`BUSINESS_WORDS` 收录了 `point→点数`、`rmb`、`usd`），于是 `POINT/RMB/USD` 被 3b 判成「字段名列表」。
- 影响方向：**信息丢失，不是英文泄露**（`LowQuality` 分支 `propose.rs:349` 把 `note` 一起置空）。所以不算安全破防。
- 真实影响：**当前 schema 无**。全库 8 条无汉字注释里没有裸 `POINT/RMB/USD`（页面上那几处都带中文前缀，如「币种单位：POINT/RMB/USD」，走 HumanReadable）。
- 判定：**建议项，不阻断**，但这是 3b 唯一一处「动了不该动的东西」，值得记一笔。

---

## 4. 变异自证独立复核（Dev 的日志不实问题）

### 4.1 BA 的发现成立，我确认了机理

`req008-mutate.sh` 里：

- `restore()` 在 **65-75 行**，用的是**裸 `echo`**（没有 `>> "$LOG"`）；
- 它被 8 处调用（142/161/179/195/211/227/243/260 行），**全部在 `run_case()` 的 `{ … } >> "$LOG"` 重定向块之外**（该块 96-107 行闭合）；
- 而 `verify_green()` 的 122 行**有** `>> "$LOG"`。

结果：日志里 `sha256` / `还原校验` **0 条**，`还原后: GREEN` 却在 —— 与 BA 观察完全一致。**代码里有校验 ≠ 校验被记录**，BA 说得对。

另外我发现一处 Dev 没提的：**`restore()` 的返回值被丢弃**（142 行等处都没接 `$?`）。所以即使 `cmp` 失败，脚本也不会因它直接计失败（只能靠后续 `verify_green` 和收尾 `git status` 间接兜住）。

### 4.2 我独立重跑：8/8 全部成立

在 `.review-workspace/audit-r8` 独立 worktree、**独立 LOG/BK 路径**（不覆盖 Dev 原日志）、**复用已编译 target**（单 target，未超 2 个上限）重跑：

| 变异 | 期望 | 实测 | 还原校验 |
| --- | --- | --- | --- |
| M1 属性名重复判据 | red | **RED ✓** | cmp 一致 `ffd39784460a64de` |
| M2 枚举值域占属性名 | red | **RED ✓** | cmp 一致 `62a426e1339d9d86` |
| M3 静默剔除 | red | **RED ✓** | cmp 一致 `ffd39784460a64de` |
| M4 丢弃率上限收到 0.1% | red | **RED ✓** | cmp 一致 `5da9272b34aab0a5` |
| M5 外键名回到注释 | red | **RED ✓** | cmp 一致 `62a426e1339d9d86` |
| M6 屈折规则被摘掉 | red | **RED ✓** | cmp 一致 `6c9a358cf57dc82f` |
| M7 字段名列表假阳性回来 | red | **RED ✓** | cmp 一致 `62a426e1339d9d86` |
| **M8 阴性对照（只加注释）** | **green** | **GREEN ✓** | cmp 一致 `6c9a358cf57dc82f` |

**通过 17 · 失败 0**，收尾 `git status` 干净。

**三重还原校验（比 Dev 的更严）**：4 个被变异文件的 sha256 与 `git show HEAD:<file>` 的 blob sha256 **逐一相等**：

```
src/domain/html_view.rs  ffd39784460a64de == HEAD  ✓
src/extract/propose.rs   62a426e1339d9d86 == HEAD  ✓
src/domain/view.rs       6c9a358cf57dc82f == HEAD  ✓
tests/drop_rate.rs       5da9272b34aab0a5 == HEAD  ✓
```

**裁定：8 次变异确实 ① 都变红 ② 还原后逐字节一致（且等于 HEAD）③ 阴性对照仍绿。**
Dev 的**结论是真的，只是他引以为据的日志证据是空的**。这是「自述不实」里最温和的一种 —— 不是编造结论，是**过程留痕造假**。

---

## 5. 页面作为判读材料够不够格（我作为评审人的实际体验）

### 5.0 先说好消息：交付物与代码一致

我用分支 HEAD 重新编译并从共享库重生成页面，与 `.review-workspace/req008-page.html` **逐字节完全一致**（94211 字节）。**交付的页面确系本 HEAD 产出，不存在「拿旧页面充数」。**

`node BA/verify-page.js` 全绿（含 jsdom 真跑搜索交互、零外部依赖、零建模术语）。我另做了一遍**独立**扫描（不信它那 7 个英文词），可见文本 12994 字。

### 5.1 40 分钟能不能做完 9 个问题

**结论：勉强能，但前提是 ★ 标记指对了地方 —— 而它没指对。**

页面上 90 个对象 / 874 个属性 / 102 条关系，3 人 × 40 分钟。真正让判读可行的机制是那 **13 个 ★ 重点对象**（`html_view` 的抽样高亮）。问题在于：

#### 🔴 阻断项 1：13 个 ★ 里有 3 个是纯技术设施

| ★ 对象 | 问题 |
| --- | --- |
| **数据库迁移版本** | 这是 alembic 迁移表，**只有 1 个属性「版本数量」**。让业务评审人判读「数据库迁移版本记录了什么」纯属浪费 |
| **鉴权缓存变更事件** | 内部鉴权缓存的事件溯源基础设施，15 个属性全是「来源事件/目标类型/重试数量/锁定操作人/最近错误」 |
| **鉴权缓存触发状态** | 同上，「主题对象类型/主题对象」 |

**3/13 的判读注意力被分给了评审人根本没有能力判断、也不需要判断的东西。** 而且它们与客户口述的业务结论毫无关系。3 人 × 40 分钟里，注意力是唯一稀缺资源。

#### 🔴 阻断项 2：★「客户」对象上 4 个「属性」根本不是属性

`customers` 对象的 15 项里有 **4 项是值域列表冒充属性名**：

| 页面上的「属性名」 | 真实列 | 评审人看到的 |
| --- | --- | --- |
| `0: 未分类, 1: 个人用户, 2: 企业用户` | `customers.type` | 不知道这一列叫**客户类型** |
| `0: 未认证, 1: 已认证` | `customers.verify` | 不知道是**认证状态** |
| `0: 已停用, 1: 已启用` | `customers.status` | 不知道是**启用状态** |
| `0: 不允许, 1: 允许` | `customers.is_foreign_model_allowed` | 不知道是**是否允许外部模型** |

- 根因：`propose.rs:693-695` `classify_comment` **先判有没有汉字**，而 `is_value_atom`（`propose.rs:493`）只收 ASCII，所以**中文值域永远走不到 EnumDomain 分支**。REQ-007 的「值域不许占属性名」只对 ASCII 值域生效。
- **全页共 63 处这种属性名**（含 ★客户意向、★客户折扣返利、★子账号额度）。
- **对 Q1/Q2 的影响是致命的**：评审人打开「客户」这一屏（★重点、37 条关系、问题最多的一屏），看到 4 个看不懂的条目。Q1「这个对象是客户吗/有哪些属性」会被答错，Q2「属性对不对」会答成「这几项看不懂」。**这正是本项目七次「假绿」里最典型的失败形态：机制测过了，材料没读过。**

#### 🟠 阻断项 3：被剔除的那个对象是**业务对象**，且告知不点名

页面顶部告知「有 1 个对象的名字带系统内部的技术叫法，没有列进来」。CLI 实际输出：

```
⚠ 有 1 个对象名带建模用语，没上页面：子账号绑定权限快照
```

根源在翻译表 `feature-REQ-008/src/extract/table_names.json:90`：

```json
"sub_account_bind_permission": "子账号绑定权限快照"
```

**这是一张真业务表**（主账号↔子账号绑定关系，含 `绑定的主账号ID` / `子账号ID`），翻译时多塞了「快照」二字，被建模用语守卫整对象滤掉。页面告知**不说是哪一个**，评审人无从知道「子账号绑定」这件事整体缺席 —— 而主账号/子账号恰好是 Q1/Q2 最可能被追问的点。

> 附带印证 briefing 的提醒：`verify-page.js` 的英文词表确实查不出「快照」这类中文建模用语，我是靠 CLI 原始输出和翻译表才定位到的。

### 5.2 哪个词最像「技术人的东西」

我的答案不是那些英文缩写，而是 **「剔除告知」本身**（见 5.4）—— 排在它前面的是 **「版本数量」**（数据库迁移版本的唯一属性，★）。

英文缩写（`API` 6 / `Token` 6 / `Provider` 5 / `IP` 10 / `JSON` 3 / `QPM` / `UA` / `PEM` / `RSA` / `LLM` / `Agent` / `POINT` / `RMB` / `USD` / `NULL` 4 / `baidu` / `mplus` / `aisearch` / `rewrite` / `im对接`）**绝大多数来自客户自己的列注释**（`模型系列(如: GPT, Claude, GLM)`、`供应商名称(如: baidu)`），业务评审人认得，**不算缺陷**。我扫出的唯一「真泄露」是 `鉴权缓存消息事件/供应商配置(base_url, api_key等)` 之类，但它们**已被剔除机制拦下**，没上页面。

### 5.3 页面上前后不一致的说法

| 不一致 | 位置 | 影响 |
| --- | --- | --- |
| 「所属客户」(33 处) vs 「主账号」/「子账号」/「划拨来源主账号」 | 跨对象 | **不算缺陷** —— 各自语义更准确（§1.2） |
| 「状态」 vs 「状态: 1-启用, 0-禁用」 vs 「0: 已停用, 1: 已启用」 | 跨对象 | 🟡 同一件事三种叫法，**问 Q2「状态字段统一吗」时评审人会说"不统一"** —— 答对的是页面的毛病 |
| 撞名 3 处 | 免费额度/免费额度、应用扣费交易/服务应用、操作日志/模拟登录会话 | 🟢 全在非 ★ 对象，且都是指向对应对象的合理外键名，**不影响 Q1/Q2** |

### 5.4 剔除告知会不会让评审人困惑

**5 处对象内告知 + 1 处全局告知**，措辞是纯中文（「另有 2 项因为名称带系统内部的技术叫法，没有显示」），本身**不会让评审人困惑**。

但它会造成**虚假安全感**：告知说「8 项」被剔除，而页面上有 **63 项**是「值域/描述冒充属性名」——那些**没被剔除、直接以属性名的身份出现**。评审人会以为「技术叫法都已经被清干净了」。这是 §5.1 阻断项 2 之所以难被发现的原因。

### 5.5 关系问句自然吗

「一个 客户，有几个 API 调用日志？」 —— **自然，无歧义**，基数直接用「1 个 / 多个」表达（`N:1`/`1:N` 记号已确认 0 出现）。

两点小意见（不阻断）：
- 句式里有半角空格：「一个 **客户**，有几个 **API 调用日志**？」 读起来略顿，但不影响理解。
- 「客户」一屏列了 **37 条**关系且全是「一个 客户，有几个 X？」。评审人要在一屏里定位自己关心的那一条，**没有分组、没有排序依据**。建议按业务主题分组或给出「重点关系」。

### 5.6 Q5 能答吗 —— 能

客户已口头确认「一个客户下面会有很多条 API 调用日志」。页面上**直接可见**：

```
[客户] ★  一个 客户，有几个 API 调用日志？  =>  多个
```

**评审人不需要自己数**。Q5 这条是达标的。（同屏另有 36 条同类问句，Q1–Q4/Q6–Q9 的关系类问题同样有现成答案。）

---

## 6. 结论

### VERDICT：**需修补**（代码合格，材料不合格）

七个自述我逐条独立复核，**7/7 成立**，没有一条编造的结论。任务 1、2、3a、3b 的修复本身**扎实、可信、有机制支撑**（词表一条没加、0 误统一、8/8 变异自证属实）。

**但判读材料不能按现状放行。** 理由不是「代码有 bug」，而是「页面把三件事摆在了 3 名非开发者评审人面前，而这三件事都会让他们答错 Q1/Q2」。

### 阻断项（判读前必须修，3 项，都很小）

| # | 问题 | 位置 | 建议修法 | 工作量 |
| --- | --- | --- | --- | --- |
| **B1** | ★ 重点对象里 3 个是技术设施（数据库迁移版本 / 鉴权缓存变更事件 / 鉴权缓存触发状态） | 抽样高亮名单 | 把这 3 个从 ★ 里去掉或换成业务对象 | 极小（改抽样名单） |
| **B2** | ★「客户」对象 4 个属性名是中文值域，全页共 63 处 | `propose.rs:693` 汉字门槛 + `propose.rs:493` `is_value_atom` 只收 ASCII | 让 `is_value_atom` / 值域判定支持中文原子，或在 HumanReadable 之前先试中文值域形态 | **中**（要防误伤 63 处里的正常中文属性名） |
| **B3** | 业务对象 `sub_account_bind_permission` 因翻译表里的「快照」二字被整对象剔除，且告知不点名 | `src/extract/table_names.json:90` | 改成「子账号绑定权限」（去掉「快照」），同步 bump `version` | **极小**（翻译表改动，但按 `names.rs:19-23` 的规矩要升 version） |

**B2 是三项里唯一有技术风险的**，建议改完重跑 `drop_rate`（上限 10%）并重新生成页面比对 —— 值域形态判定放宽最容易连带误杀正常属性名。

### 建议项（不阻断，但建议记入下一轮）

| # | 问题 | 证据 |
| --- | --- | --- |
| S1 | `is_field_name_list` 的剩余漏洞比文档记的宽：`0: name, 1: email`、`code,name`、`url,name` 都会泄露英文 | §3.3；`propose.rs:509` `has_code_prefix` + `view.rs:470` `code`/`url` 不在词表 |
| S2 | 3b 把 `POINT/RMB/USD` / `RMB/USD` 从 EnumDomain 打成 LowQuality，值域信息丢失（fail-closed 方向，当前 schema 无影响） | §3.4 |
| S3 | `view.rs:400-402` 文档宣称有 `-is` 守卫，`view.rs:444` 代码里没有；`analysis` 靠词表未命中碰巧对 | §2.4 |
| S4 | 屈折只支持单层词元，`original_prices` / `cooling_hours` 仍在丢弃名单 | §2.5（已文档化+已测试，属取舍） |
| S5 | `tests/test_fk_naming.rs` 硬编码绝对路径 `/home/myg133/.../schema.json`，与 `tests/drop_rate.rs:29` 的相对路径约定不一致 —— 换机器/CI 上 `cargo test` 会 panic | 两文件对照 |
| S6 | 变异脚本 `restore()` 的返回值被丢弃，且其输出不进日志（§4.1） | `req008-mutate.sh:142` 等 8 处 |
| S7 | 「客户」一屏 37 条关系无分组/无排序，评审人定位困难 | §5.5 |

### 我没能验证的部分

诚实列出，以下均**未做**或**无法做**：

1. **P1 全部未做**：未验证 90 个对象的逐屏文案是否都对非开发者友好（只精查了 13 个 ★ + 撞名 + 值域三类）；未逐条核对 102 条关系的语义正确性（只验证了基数呈现与 Q5）。
2. **未做 develop 基线编译**：`8.4%` 是**推算**（75+6=81，81/964=8.4%），不是实跑 develop 得到的。我用「4 张词表与 develop 内容逐条相同 + 6 条救回列已定位」来等价证明，未实际编译 develop 跑一遍 drop_rate。
3. **未验证共享库里 `e2e_req008b` 之外的项目**，也未验证从「真实客户源库 → 提案 → 审核 → 冻结 → 客户视图」**全链路**端到端只跑了一遍 `ontology show`（因为不得接触客户源库）。页面的上游提案正确性（审核轨道、冻结快照）不在本次范围。
4. **未验证 3 名真实评审人的 40 分钟实测**。§5 的判断是我作为「评审人之一」的模拟体验，不能替代真人计时。
5. **未验证 `verify-page.js` 未覆盖的交互**（如浏览器真实渲染、打印/移动端、折叠状态的持久化）—— 该脚本用 jsdom，非真浏览器。
6. **未审计 `table_names.json` 全部 91 条翻译**（只定位到 `:90` 一条因「快照」被剔除的）。翻译质量是判读材料的隐性支柱，值得单独一轮。
7. **未验证并发/规模**：只测了单 schema 全量，没有性能或更大库的行为。
8. **`req008-drop-after.txt` 有 77 行但其中 1 行是 cargo 输出、实为 76 条，而 `drop_rate` 报 75 条** —— 差 1 条我没追下去（该文件是 Dev 的产物，不影响 `cargo test` 的结论，但说明这份手工清单与测试口径略有出入，**存疑**）。
