# REQ-144 阶段 1b · F1–F5 增量 QA 审核

> **审核对象**：`feature-REQ-144b` · 分支 `feature/REQ-144-stage1`
> **本轮增量**：`b98eb83`（Dev：F1–F5，F4 把 `request` 换成通用模板）· `5476ac3`（BA：真实脱敏夹具）
> · `37a95fa`（BA：fixtures/README 登记）
> ⚠️ **审核期间 HEAD 又前进了一个提交**：`844fb5b`（BA：README 补充「来源账号 = 验证账号」）。
> 该提交**只改 `fixtures/README.md` 7 行**，不动代码/夹具 ⇒ 本报告的全部判据对它同样成立，
> 但「3 个提交」这个口径已经变成 **4 个**，请 BA 在自己的状态里对齐。
> **基线**：`develop` `6f4987d`（自建二进制，`git archive`，不从 Dev 处取）
> **上一次 PASS 的范围**：`1d48791`。本轮不继承那次的 PASS —— F4 是结构性替换，重新验。
> **审核方式**：不服单测。F4 的三种 body 形状由**我自己写契约**、打到**我自己起的假端点**、
> 用 CLI 黑盒看服务端**实际收到什么**；并用 `hashlib/hmac` 独立复算 `Authorization`
> （证明「签的 body == 发的 body」）。F3 的错误分类用同一个假端点 + 真实不可达地址；
> 变异改在 `.local/` 下的**源码副本**上（⛔ 不碰 feature 分支）。
> **证据目录**：`feature-REQ-144b/.local/qa144s1b/`（⛔ 不在 `%TEMP%`）
> **二进制**：feature `ac856506…`（25,900,032 B）· 基线 `0fe00e41…`（25,670,144 B）
> **审核时间**：2026-10-25 · QA agent `qa-req144-stage1b`
> **feature worktree 状态**：`git status --porcelain` **空**（全程只读，未改一行）

---

## 1 · 结论

> ## **FAIL —— 阻断 1 条（真实夹具残留 1 个真实资源标识）**
>
> 代码增量（F1–F5）本身**全部通过**：F4 的三种形状/变异/三条剪枝规则全对，F3 五类错误
> + 变异全对，F5 逐字节落盘 + 误用不写文件，F1 注释与全链测试到位，零回归 7/8（第 8 个是
> 已裁定的 R-C 文案）。**FAIL 只来自 P0-2：`fixtures/tencent-cvm-describe-real.json` 里有一个
> 真实资源标识没脱敏（`LoginSettings.KeyIds[0]`，与真调用原文逐字节相同）。**
> 这是**一行修复**（把 `skey-…` 换成占位符 + 补进 README 的脱敏范围 + 加一条防回归扫描），
> 但按本轮 P0 的字面判据（「真实夹具不许有真实资源标识」）不能给 PASS。

| # | P0 项 | 结果 |
| --- | --- | --- |
| 1 | F4 通用请求模板：三种形状 + 变异 + 剪枝 + 零厂商名词 | ✅ 行为全对；⚠️ 字面 grep **不是 0**（见 §2.1，非阻断） |
| 2 | 真实夹具脱敏完整性（52 字段 / 3 台 / TotalCount=3） | ❌ **阻断**：`LoginSettings.KeyIds[0]` = 真实 `skey-` ID；结构 52/3/3 全对 |
| 3 | 两份夹具分工是否混用 | ✅ 无混用（真实夹具**零自动化引用**，是覆盖缺口不是误用） |
| 4 | F5 `--dump-raw`：仅真调用 / 原文落盘 / 误用不写 / 无新写点 | ✅ 4/4 |
| 5 | F3 五类错误 + 变异 | ✅ 8 场景全对；变异**红**；⚠️ 一个 5xx 边界（见 §6.3） |
| 6 | F1 注释已改事实 + 测试走 `sign()` 全链 | ✅ 逐字贴出，见 §7 |
| 7 | 零回归 + 阶段 0 九条 | ✅ 7/8 + 已裁 R-C；九条 9/0/0；三列基线见 §8 |

**阻断（必须修）**

| # | 级别 | 位置 | 问题 |
| --- | --- | --- | --- |
| **B-1** | **阻断** | `fixtures/tencent-cvm-describe-real.json:53` | 第 1 台实例的 `LoginSettings.KeyIds[0]` = **`skey-q6r85k71`**，与 `.local/req144-stage1/real-dump/describe-instances.raw.json` **逐字节相同** ⇒ 真实密钥对 ID 进了仓库；README 的脱敏范围（`fixtures/README.md:45`）**没有 `LoginSettings.KeyIds`**，`fixtures/README.md:47` 却写「真实 ins- ID 残留 0」（窄口径为真，整体口径为假） |

**非阻断（建议下一轮做）**

| # | 位置 | 问题 |
| --- | --- | --- |
| N-1 | `src/cli.rs:836-855` | `access fetch --help` 还在教**已经被删掉的** `request.from_filter` 三种写法（`{"scalar":…}` / `{"array":…,"item":…}`）。实测这些写法现在被解析器**直接拒绝**（`ACCESS_SCHEMA_INVALID`，「兼容写法只接受字符串」）⇒ 照抄 help 的人必失败 |
| N-2 | `fixtures/README.md:44,49` | README 把脱敏脚本登记为 `.local/req144-stage1/scripts/desensitize.py`（并称「确定性」），**该文件不存在**（`.local/` 下全盘搜不到）⇒ 脱敏不可复现，也解释了 B-1 为什么漏（范围是手写的） |
| N-3 | `src/access/net.rs:451` | `classify_http` **先看信封再看状态码**：HTTP 500 + `{"Response":{"Error":{"Code":"InternalError"}}}` 实测**只打 1 次、不重试**（规格写的是「5xx 重试到上限」）。纯 500（无信封）实测 3 次，正常。真实网关 5xx 一般不带业务信封，影响低，但判据与实现有缝 |
| N-4 | `fixtures/tencent-cvm-describe-real.json` | 没有任何测试引用它（也没有引用 `access-cvm-real.json`）⇒ 「真实夹具用于字段校准」目前**只靠手跑** `access validate`。我跑了（`ok:true`，5 字段 3/3），但它不会在 CI/回归里红 |
| N-5 | `src/access/net.rs:90,92`；`src/access/*` 多处注释/测试 | 「零厂商名词」按字面不成立：`TENCENTCLOUD_SECRET_ID/KEY` 是**运行时**常量（契约没写 `credential_env` 时的兜底），`X-TC-*` 是运行时头名（`auth.mode=tc3-hmac-sha256` 的协议要求），`Zone` 24 处（全在注释/错误示例/单测）。**没有任何 `if vendor == …` 分支**，请求体确实零特例 |

---

## 2 · F4「通用请求模板」真的通用吗（P0-1）

### 2.1 零厂商名词：字面 grep **不是 0**

| grep（对 `src/access/`） | 命中 | 判定 |
| --- | --- | --- |
| `Filters` | **0** | ✅ |
| `tencent`（小写） | **0** | ✅（但大写不算 —— 见下） |
| `DescribeInstances` | **0** | ✅ |
| `Zone`（大小写敏感） | **24** | ⚠️ **不是 0** |

`Zone` 24 处的**逐条分类**（没有一处在运行时判定逻辑里）：

| 位置 | 性质 |
| --- | --- |
| `mod.rs:52,373,727`、`net.rs:1012`、`path.rs:7`、`validate.rs:31` | 模块/字段文档（`Placement.Zone` 作为路径示例） |
| `mod.rs:1011,1020,1029,1045` | 旧写法兼容的注释 + **错误信息里的示例字符串**（`{"zone":"Zone"}`） |
| `mod.rs:1737-1834`（8 处）、`fetch.rs:694,738`、`path.rs:90,91,100`、`validate.rs:640` | `#[cfg(test)]` 单测 / 测试夹具构造 |

另外两条 dev 的 grep **覆盖不到**的厂商名词（大小写/协议层）：

| 位置 | 内容 | 是否运行时 | 说明 |
| --- | --- | --- | --- |
| `net.rs:90,92` | `pub const CANONICAL_SECRET_ID_ENV = "TENCENTCLOUD_SECRET_ID"` | **是** | 契约 `auth.credential_env` 没写时的**兜底变量名**；有声明时优先用声明的（我实测：只设 `QA144_SECRET_*`、不设 `TENCENTCLOUD_*`，真调用照样成功 ⇒ 兜底没被触发） |
| `net.rs:784-798` | `X-TC-Action / X-TC-Version / X-TC-Timestamp / X-TC-Region` | **是** | 与 `auth.mode=tc3-hmac-sha256` 绑定的协议头，换签名方式才需要动 |
| `net.rs:533` | 错误文案里的 `TENCENTCLOUD_SECRET_ID/KEY` | 是（文案） | 可行动提示，不影响判定 |

**结论**：请求体/过滤这一层**确实零厂商名词、零厂商分支**（这是 F4 的核心）；但「字面零厂商名词」
不成立。要达标只有两条路：① 把 `TENCENTCLOUD_*` 兜底去掉（强制契约声明 `credential_env`），
或 ② 承认这是 `auth.mode` 的协议常量并写进「已知限制」。**不建议**为了 grep 好看去改注释/单测里的
`Placement.Zone` —— 那是路径示例，不是分支。

### 2.2 三种 body 形状（**契约是我写的，不是它的测试**）

假端点记录服务端**实际收到的 body**；再对每个请求用 Python `hashlib/hmac` 按文档四步**独立复算**
`Authorization`（secret 用我自己的假值），与服务端收到的头逐字节比 —— 这一条证明的不是「签名函数对」，
而是「**签进去的 payload 就是发出去的 body**」。

| 形状 | 我写的 `request.template` | 服务端实际收到（除分页外逐字） | 独立复算签名 |
| --- | --- | --- | --- |
| ① 顶层标量 + 顶层数组 | `{"Region":"{region}","InstanceIds":"{ids}"}` | `{"InstanceIds":["i-1","i-2"],"Limit":2,"Offset":0,"Region":"ap-guangzhou"}` | ✅ 一致 |
| ② 数组里放对象 | `{"Filters":[{"Name":"zone","Values":["{zone}"]}]}` | `{"Filters":[{"Name":"zone","Values":["ap-guangzhou-5"]}],"Limit":2,"Offset":0}` | ✅ 一致 |
| ③ 三层嵌套 | `{"Cond":{"Rules":[{"Field":"zone","Value":"{zone}"}]}}` | `{"Cond":{"Rules":[{"Field":"zone","Value":"z-9"}]},"Limit":2,"Offset":0}` | ✅ 一致（形状③逐字节断言） |

### 2.3 变异：拆掉一层嵌套 ⇒ body 必须跟着变

同一份契约只把 `Cond.Rules` 从**数组**改成**对象**（`[{"Field":…}]` → `{"Field":…}`）：

```
deep : {"Cond":{"Rules":[{"Field":"zone","Value":"z-9"}]},"Limit":2,"Offset":0}
flat : {"Cond":{"Rules":{"Field":"zone","Value":"z-9"}},"Limit":2,"Offset":0}
```

✅ body 跟着模板变 ⇒ 不是「某个特例分支恰好长这样」。

另验：`request.paging = {"offset":"Page.Number","limit":"Page.Size"}` ⇒
`{"Page":{"Number":0,"Size":2},"Region":"ap-guangzhou"}` —— 分页值是**按路径写**的，不写死顶层键。

### 2.4 剪枝规则（逐条）

| 规则 | 我给的模板 / 条件 | 服务端实际收到 | 判定 |
| --- | --- | --- | --- |
| 对象成员缺值 ⇒ 丢键 | `{"Region":"{region}","OptionalZone":"{zone}"}`，不给 `--filter` | `{"Limit":2,"Offset":0,"Region":"ap-guangzhou"}`（`OptionalZone` **整键消失**，不是空串） | ✅ |
| 数组元素子树有剪枝 ⇒ 整段丢；数组空了连键丢 | `{"Filters":[{"Name":"zone","Values":["{zone}"]}],"Keep":"const"}`，不给 `--filter` | `{"Keep":"const","Limit":2,"Offset":0}`（`Filters` 整键消失；`Keep` 保留） | ✅ |
| 常量数组不许被误剪（反向对照） | `{"Filters":[{"Name":"fixed","Values":["always"]}]}` | `{"Filters":[{"Name":"fixed","Values":["always"]}],…}` | ✅ |
| 文本里**嵌着**占位符缺值 ⇒ 报错、不发请求 | `{"Text":"pre-{zone}"}`，不给 `--filter` | **0 个请求**；`exit=1`；`REQUEST_BINDING_MISSING` | ✅ |

### 2.5 旧写法兼容（顺手验的）

`{"from_filter":{"zone":"Zone"}}` 仍可用（解析时内部转模板），且 `template` + `from_filter` 同时写
会报 `ACCESS_SCHEMA_INVALID`。**但** `--help` 里那两种对象写法现在不认了 —— 见 N-1（我实测
`{"array":"Filters",…}` ⇒ `ACCESS_SCHEMA_INVALID · 兼容写法只接受字符串`）。

---

## 3 · 真实夹具的脱敏完整性（P0-2，我自己扫的）

### 3.1 方法（不采信它的脚本/结论）

原始响应在 `.local/req144-stage1/real-dump/describe-instances.raw.json`（5,759 B，`git check-ignore`
确认**未入库**）。我写 `.local/qa144s1b/scan_fixture.py`：把原文与脱敏版**按叶子路径逐字段对齐**，
对**身份类字段**判「masked 是否仍等于 raw」（整值相等 / 互为子串都算残留），并校验占位符形态。
输出：`.local/qa144s1b/fixture-scan.out.txt`（真实值一律不打印，只打字段名与判定）。

### 3.2 结构一致性：**全对**

| 项 | 原文 | 夹具 | 判定 |
| --- | --- | --- | --- |
| 实例数 / `TotalCount` | 3 / 3 | 3 / 3 | ✅ |
| 每台字段数 | 52 / 52 / 52 | 52 / 52 / 52 | ✅ |
| 字段名 + **顺序**（3 台一致） | 是 | 是 | ✅ 与原文**完全相同** |
| `Response` 顶层键 | `TotalCount / InstanceSet / RequestId` | 同 | ✅ |

### 3.3 可疑值逐个判（这是本轮要的「列出来 + 结论」）

| 字段 | 夹具里的值（形态） | 判定 |
| --- | --- | --- |
| `InstanceId` ×3 | `ins-0000001..3` | ✅ 符合 `ins-000000N`，与原文全不同 |
| `InstanceName` ×3 | `demo-cvm-01..03` | ✅ 合成名（原文实例名已全部替换） |
| `PrivateIpAddresses` / `PublicIpAddresses` | `10.0.1.0` / `10.0.3.0` … | ✅ 全在 `10.0.x.x`（**原文的公网 IP 也已换成 10.0.x.x**，这点做对了） |
| `Uuid` | `00000000-0000-4000-8000-…000N` | ✅ 全零 + 版本位，非真实 UUID |
| `OperatorUin` | `100000000000`（三台同值） | ✅ 与原文不同（占位）；⚠️ 形状仍是 12 位数字，是全表唯一「形似真值」的占位，可接受 |
| `SystemDisk.DiskId` | `disk-000000N` | ✅ |
| `VirtualPrivateCloud.VpcId/SubnetId`、`SecurityGroupIds`、`ImageId` | `vpc-/subnet-/sg-/img-000000N` | ✅ |
| `LatestOperationRequestId`、`RequestId` | `opreq-0000000N`、`req-000000000001` | ✅（原文是 UUID） |
| `CamRoleName` | `demo-cam-role`（原文空） | ✅ 无泄漏 |
| `OsName` / `InstanceType` / `Zone` / 时间 | 保留 | ✅ README 声明的「保留语义」范围，公开目录值 |
| **`LoginSettings.KeyIds[0]`（仅第 1 台；另两台 `null`）** | **`skey-q6r85k71`** | ❌ **与原文逐字节相同 = 真实密钥对 ID 残留** |

**B-1 的硬证据**：`scan_fixture.py` 的身份类字段比对里**唯一**一条 `[RESIDUAL]` 就是它；
子串扫描也只在它身上命中（长度 13/13 完全相等）；全文 ID 前缀扫描里，`ins-/disk-/vpc-/subnet-/
sg-/img-/opreq-/req-` **全部是全零占位**，只有 `skey-` 是随机串。`skey-` + 8 位小写字母数字
正是腾讯云密钥对 ID 的真实形态，且原文这台机器确实挂了这把 key。**这不是「合成得不像」，是原文照抄。**

### 3.4 README 的登记与事实的差

| README | 事实 |
| --- | --- |
| `:44` 脱敏脚本 `.local/req144-stage1/scripts/desensitize.py`（确定性） | **文件不存在**（`.local` 全盘 `Get-ChildItem -Filter desensitize*` = 0 命中）⇒ 无法复跑，也无法审计「范围为什么漏了 KeyIds」 |
| `:45` 脱敏范围（12 项） | **漏了 `LoginSettings.KeyIds`**（还有 `DiskId` / `LatestOperationRequestId` / `CamRoleName`，但这三项的值已换成占位符，不算残留） |
| `:47` 「真实 `ins-` ID 残留 0」 | 字面为真（`ins-` 确实 0），但**整体口径为假**：还有 1 个真实 `skey-` 资源标识。这句话会让下一个人**停止扫描** |
| `:49` 「重采必须跑 `desensitize.py`」 | 脚本不存在 ⇒ 这条纪律事实上执行不了 |

**修复建议（B-1）**：① `KeyIds[0]` → `skey-0000001`（保持数组/字段形状）；② README 脱敏范围补
`LoginSettings.KeyIds`；③ 把 `desensitize.py` 真正放进 `.local/req144-stage1/scripts/`（或写明它是一次性
脚本、并给出等价规则）；④ 加一条**防回归**：对 raw 与 masked 逐字段断言「身份类字段不得相等」——
这条能自动抓住下一个漏网字段，而不是靠人眼。

---

## 4 · 两份夹具是否被混用（P0-3）

| 检查 | 结果 |
| --- | --- |
| 有没有测试拿**真实夹具**测分页 | **没有**。`git grep 'describe-real\|access-cvm-real'` 在 `tests/` / `src/` 里 **0 命中** |
| 真实夹具被谁引用 | 只有 `fixtures/access-cvm-real.json:118` 的 `fixture` 字段；那份 mapping **也没有任何测试引用** |
| 合成夹具的用途 | `tests/test_req144_access.rs::ac004_paging_and_local_filter` 用它测 **250 行 / 3 页**（`meta.pages == 3`）、本地过滤 83 条 ⇒ 分工正确，没被真实夹具污染 |
| 真实夹具的「字段校准」是否真的能跑 | 我手跑：`ontok access validate --mapping fixtures/access-cvm-real.json --ontology fixtures/access-ontology.ttl` ⇒ `ok:true`、`fixture_rows:3`、`field_coverage` 5 个字段全 `3/3`、0 error / 0 warning ✅ |

**结论**：**没有混用**。但真实夹具**零自动化引用** ⇒ 它被换坏/被改回合成形状时没有任何测试会红
（N-4）。建议加一条「`validate(access-cvm-real.json)` 必须 ok 且 5 字段 3/3」的测试，成本很低。

---

## 5 · F5 `--dump-raw`（P0-4）

| 判据 | 我的实测 | 判定 |
| --- | --- | --- |
| 只在 `--allow-network` 时有效 | `--dry-run-fixture --dump-raw <path>` ⇒ `exit=1`、stderr「`--dump-raw` 只对真调用…」、**文件不存在**、假端点 **0 请求** | ✅ |
| 同上（两个开关都不给） | `--dump-raw <path>` 单独给 ⇒ 在 `DRY_RUN_REQUIRED` 那一步就拒，文件不存在 | ✅ |
| 指向目录 | ⇒ `exit=1`「它要的是一个文件路径」、**0 请求** | ✅ |
| 落盘 = 服务端**原文** | 假端点故意返回**非字母序键序** + `\uXXXX` 转义 + 非 ASCII（`naïve`）的 220 B 响应；落盘 220 B，`sha256` **两侧完全相同**；`"RequestId":"req-qa-0001","TotalCount":1` 的键序、`\\u4e2d\\u6587` 的转义、UTF-8 原文**都原样保留** | ✅ |
| 不裁剪 | 同上（完整 `Response.InstanceSet[]`） | ✅ |
| 没有新增 fs 写点 | `dump` 走的是**既有** `write_local`（`cli.rs:2874`，publish 的 I31 同一个物理写点）；`git diff 6f4987d..HEAD -- src/domain/pipeline.rs tests/test_req107_outlet_pipeline.rs tests/test_req107_red_lines.rs Cargo.toml Cargo.lock` **为空**；`test_req107_source_enumeration` 的 +5 行只是 **MODULES 登记 `net`/`tc3`**，明确写了「不新增出口」 | ✅ |

---

## 6 · F3 错误分类（P0-5）

### 6.1 五类 + 三个附加场景（CLI 黑盒，假端点记录**实际请求次数**）

| 场景 | 期望 | 实测 code | 请求次数 | 判定 |
| --- | --- | --- | --- | --- |
| `connect` 阶段超时（不可路由 `10.255.255.1:81`，connect 2s / global 5s） | `UPSTREAM_UNREACHABLE` | `UPSTREAM_UNREACHABLE`（`timeout: connect`，文案指向安全组/端口/DNS） | —（连不上） | ✅ **不是** TIMEOUT |
| IO 错误：真实 `127.0.0.1:1`（端口未开） | `UPSTREAM_UNREACHABLE` | `UPSTREAM_UNREACHABLE` | — | ✅ |
| `wait` 阶段超时（连上后服务端 sleep 5s，global 2s） | `UPSTREAM_TIMEOUT` | `UPSTREAM_TIMEOUT` | 3 | ✅ 重试有上限 |
| HTTP 401（带 `AuthFailure` 信封） | `AUTH_REJECTED`，不重试 | `AUTH_REJECTED` | **1** | ✅ |
| HTTP 401（**无信封**，只有状态码） | `AUTH_REJECTED`，不重试 | `AUTH_REJECTED` | **1** | ✅ |
| HTTP 400 | `API_ERROR`，不重试 | `API_ERROR` | **1** | ✅ |
| HTTP 500（纯文本） | 重试到上限 ⇒ `API_ERROR` | `API_ERROR` | **3** | ✅ |
| HTTP 429（首次 429，随后 200） | 退避后成功 | `exit=0` | **2** | ✅ |
| `auth.mode=none` | `AUTH_MODE_UNSUPPORTED`，**发请求之前** | `AUTH_MODE_UNSUPPORTED` | **0** | ✅ |

汇总证据：`.local/qa144s1b/net-cases.log`（**24/24 PASS**），每次请求的 body/headers/`Authorization`
落在 `.local/qa144s1b/cases/*.stderr.txt` + 脚本内的 records。

### 6.2 变异：把 401 改成可重试 ⇒ 必须红

在 `.local/qa144s1b/mut401-src/`（`git archive HEAD` 的副本，**feature 分支未动**）把
`net.rs` 的两处 401 判定改成 `Verdict::Retry(...)`，跑它的 `test_req144_network::auth_rejected_is_not_retried`：

```
test auth_rejected_is_not_retried ... FAILED
panicked at tests\test_req144_network.rs:814:5:
assertion `left == right` failed: API_ERROR · 服务端返回码 `FakeScriptExhausted` ...
  left: "API_ERROR"   right: "AUTH_REJECTED"
test result: FAILED. 0 passed; 1 failed
```

✅ **变红**（`cargo` 退出码 101），红在预期判据上（重试导致脚本耗尽 ⇒ 不再是 401 的拒绝）。
证据：`.local/qa144s1b/mut401-test.log`。

### 6.3 我顺手发现的一条：**状态码 401 的测试盲区**（非阻断）

第一次变异我只改了 `classify_http` 的 `401 | 403 =>` 分支，测试**仍然绿**。原因：它的 401
用例的响应体带 `{"Response":{"Error":{"Code":"AuthFailure.SignatureFailure"}}}`，而
`classify_http` **先看信封**（`net.rs:451`），真正的判定点是 `classify_api_error_code`
（`net.rs:511`），状态码分支根本走不到。⇒ **`status-only 401` 这条路径在它的测试里没有覆盖**
（我用自己的 CLI 补了这一条，见 §6.1；结果是 1 次请求 + `AUTH_REJECTED`，行为正确）。
建议把 401 用例拆成「有信封 / 无信封」两条。

**N-3（5xx + 信封）**：`net.rs:451` 先看信封意味着 500 带 `Response.Error` 时走
`classify_api_error_code` ⇒ `Fatal`，**不重试**（实测 1 次）。纯 500 是 3 次。要不要按状态码优先，
是个产品判断；我把它记为**非阻断**，但规格文字（「5xx 重试到上限」）和实现对不上。

---

## 7 · F1 注释已改成事实（P0-6）

`src/access/tc3.rs:41-55` 原文（逐字）：

```
41: //! # 文档里那串打码的 SecretKey，其实是**能跑的**
43: //! 文档写的是 `SecretKey = "********************************"`。它**不是省略号**，而是
44: //! **字面 32 个星号**：把这 32 个 `*` 当 SecretKey 喂进 [`sign`]，能逐字节复现文档公布的
45: //! `SecretDate` / `SecretService` / `SecretSigning` 三把派生密钥与最终 `Signature`。
47: //! ⇒ **派生链的第一跳也可对拍**，不存在「SecretKey 被打码，所以只能从中间密钥接上」这种事。
48: //! `tests/test_req144_tc3_vectors.rs` 的 `official_vector_1_*` 走的就是 **`sign()` 全链**
49: //! （不是从中间密钥接上），并逐项断言文档公布值；[`tests::official_vector_1_derived_key_chain_replays`]
51: ```
53: //! ⛔ 本文件曾经写过一句错话（已删）：『SecretKey 打码 ⇒ 派生链第一跳无从对拍』。
54: //!    那句话会让下一个人以为「这段没法验」从而**根本不去验** —— 而整条链都能验。
55: //! ```
```

测试名与位置（逐条核对过）：

| 测试 | 位置 | 全链证据 |
| --- | --- | --- |
| `official_vector_1_cvm_1551113065_is_byte_identical` | `tests/test_req144_tc3_vectors.rs:78` | `doc_input()` 直接给 `secret_key: "********************************"`（`:63`）⇒ `let signed = sign(&input)`（`:88`）⇒ 断言 `SecretSigning`（`:92`）、`canonical_request`、`hashed_canonical_request`、`string_to_sign`、`signature`、`authorization` |
| `official_vector_1_derived_key_chain_replays` | `:119` | 用文档公布的三把十六进制密钥逐级回放（第二种验法） |

`git grep '无法对拍\|打码'` ⇒ 只剩上面这三段**说明「已删/能跑」**的元叙述，**没有**任何一句还
声称「验不了」。**F1 ✅**（我上一轮的两条要求：删错话 + 走 `sign()` 全链，都落实了）。

---

## 8 · 零回归 + 阶段 0 九条（P0-7）

### 8.1 八 case 逐字节（feature HEAD vs 自建基线二进制 `6f4987d`）

方法：同一份输入、同一个 cwd、只换二进制；`fetched_at` 与 stderr 的「时点」回显归一化；
`publish` 的 `--out` 路径回显归一化（两侧目录不同，不是代码差异）；其余 stdout/stderr/退出码/
publish 产物**逐字节**比。脚本 `.local/qa144s1b/regression.ps1`，结果 `regression/summary.json`。

| # | case | 退出码 f/base | stdout sha(f)=sha(base) | stderr 相同 | 判定 |
| --- | --- | --- | --- | --- | --- |
| 1 | fetch `{"status":"STOPPED"}` `--limit 50`（本地过滤） | 0/0 | `a75680f2…` ✅ | ✅ | 逐字节相同 |
| 2 | fetch `--entity alarm --limit 10`（无过滤） | 0/0 | `968f931d…` ✅ | ✅ | 逐字节相同 |
| 3 | fetch `{"zone":"ap-guangzhou-2"}` `--limit 200`（下推+截断） | 0/0 | `7f404e70…` ✅ | ✅ | 逐字节相同 |
| 4 | validate（好契约） | 0/0 | `312c16b0…` ✅ | ✅ | 逐字节相同 |
| 5 | validate（R4 重叠的坏契约） | 1/1 | `b5033773…` ✅ | ✅ | 逐字节相同 |
| 6 | publish `--out <同一路径>` | 0/0 | （空） | ✅（路径归一后） | 产物 `324624a5…` **两侧相同**（7093 B） |
| 7 | fetch 不给任何开关 | 1/1 | （空） | ⚠️ 不同 | **仅 R-C 已裁文案**（见下） |
| 8 | fetch `{"bogus":"x"}`（未知过滤名） | 1/1 | （空） | ✅ | 逐字节相同 |

**7/8 逐字节相同**，第 8 个是**上一轮已裁定接受的 R-C 文案变更**，逐字如下：

```diff
- ✗ DRY_RUN_REQUIRED · 阶段 0 **不发任何网络请求**：请显式加 `--dry-run-fixture` 用夹具跑。
- 真实调用（TC3 签名 / 凭据 / 限流退避）是阶段 1 的事，本程序里**没有**那条路径。
+ ✗ DRY_RUN_REQUIRED · 请显式选一条通路：`--dry-run-fixture`（读本地夹具，⛔ 不发网络请求）或 `--allow-network`（真调用：TC3 签名 / 凭据 / 限流退避）。
+ ⛔ 两个都不给时本程序既不联网，也不静默降级成「读 fixture 冒充真数据」。
```

（原文案在阶段 1 落地后**已经是假话**，必须改；码与退出码未变。）另外 `meta.network` 栏在
fixture 通路确实**不出现**（case 1/2/3 stdout 逐字节相同即硬证据）。

### 8.2 指定套件：passed / failed / **SKIP** 三列

> 新规程：**SKIP 不许记 passed**。`test_req145_live` 的 9 条在我这个环境里全部打印
> `*** SKIP: … 缺 ONTOK_SOURCE_DATABASE_URL … 这条判据没有执行，不是通过 ***`，
> 虽然 cargo 的汇总行写「9 passed」，但**实际执行数 = 0**。下表按真实执行计。

| 套件 | feature（P/F/SKIP） | 基线 `6f4987d`（P/F/SKIP） | 说明 |
| --- | --- | --- | --- |
| `test_req144_access`（阶段 0 九条） | **9 / 0 / 0** | 9 / 0 / 0 | 全绿 |
| `test_req144_network`（F3/F4/F5） | **26 / 0 / 0** | 目标不存在 | 新增，全绿 |
| `test_req144_tc3_vectors`（F1） | **6 / 0 / 0** | 目标不存在 | 新增，全绿 |
| `test_req107_red_lines`（四红线） | **2 / 0 / 0** | 2 / 0 / 0 | 全绿 |
| `test_req107_source_enumeration` | **10 / 0 / 0** | 10 / 0 / 0 | 全绿（含 `net`/`tc3` 模块登记） |
| `test_req145_live` | **0 / 0 / 9（SKIP）** | 0 / 0 / 9（SKIP） | 缺 `ONTOK_SOURCE_DATABASE_URL` / `ONTOK_REQ145_SCHEMA` / `ONTOK_REQ145_TENANT`；两边同 skip ⇒ **不是回归** |

**关于 `test_req145_live` 的诚实说明**：我试过把环境补齐（源库 `172.16.120.20:30432/anspire_open_uat`
+ `code/out/schema.json` + tenant），结果 9 条**全失败**，原因是**环境对不上**：任务给的元数据库里
没有 `anspire_r2rml_excl`（`project list` 只有 `ops360` / `ops360-live` / …），换 `ops360` v2 后
又因快照表（`doc_*`）与 `code/out/schema.json` 不匹配而在 `build_service` 前置失败。⇒ 这条套件在
**本环境无法执行**，不是本提交引入的红；上一轮报的「9/9」如果也是这种 skip，应当按新规程改写。
（证据：`.local/qa144s1b/req145-live-attempt.log` / `-attempt2.log`。）

---

## 9 · 临时目录与清理

| 项 | 值 |
| --- | --- |
| 证据根 | `feature-REQ-144b/.local/qa144s1b/`（脚本、日志、24 个 case 的产物、8 case 对比、变异日志） |
| 构建目录 | `.local/cargo-qa144s1b/`（feature，**保留**）· `.local/cargo-qa144s1b-base/` + `.local/cargo-qa144s1b-mut/`（**审核后删除，约 8.3 GB**） |
| 基线源码/二进制 | `.local/qa144s1b/baseline-src/`（`git archive 6f4987d`）+ `baseline.tar` |
| 变异源码 | `.local/qa144s1b/mut401-src/`（**副本**，feature 分支未被修改） |
| `%TEMP%` | 我的证据**一个也没有**。⚠️ 但 Dev 的 `test_req144_network` 每次跑完会在 `%TEMP%` 留 `ontok-req144-net-*` 目录（`std::env::temp_dir()`，本次留下 ~200 个）—— 这是**测试自身的行为**，不是我的证据；建议下轮改成 `tempfile` 或在 `Drop` 里清理 |
| feature 分支 | `git status --porcelain` **空**；未 commit / 未 push |

> 清理动作只删我自建的两个辅助 target（路径已核对为
> `D:\MyCodes\AI\an_onto_demo\feature-REQ-144b\.local\cargo-qa144s1b-{base,mut}`）。
> 证据与主 target 保留，供复验。

---

## 10 · 复现命令（关键几条）

```powershell
# F4/F3/F5 全量（24 个判据，自己起的假端点 + CLI 黑盒）
python .local\qa144s1b\net_cases.py          # → .local/qa144s1b/net-cases.log

# 真实夹具脱敏扫描（不打印真实值）
python .local\qa144s1b\scan_fixture.py       # → .local/qa144s1b/fixture-scan.out.txt

# 零回归 8 case（基线二进制已随 §9 的清理删除；先按需从 .local/qa144s1b/baseline-src 重建）
cargo build --bin ontok --manifest-path .local\qa144s1b\baseline-src\Cargo.toml   # -> 另起 target
pwsh -File .local\qa144s1b\regression.ps1 -FeatureBin .local\cargo-qa144s1b\debug\ontok.exe `
     -BaseBin <重建出的 baseline ontok.exe> -Out .local\qa144s1b\regression
# 已有结果可直接看 .local/qa144s1b/regression/summary.json（8 个 case 的 sha + publish sha）

# F3 变异（在副本上）
cd .local\qa144s1b\mut401-src; cargo test --test test_req144_network auth_rejected_is_not_retried
```

---

## 11 · 给下一轮的最小修复清单

1. **B-1（阻断）**：`fixtures/tencent-cvm-describe-real.json` 的 `LoginSettings.KeyIds[0]` → 占位符；
   README 脱敏范围补 `LoginSettings.KeyIds`；补 `desensitize.py` 或登记它是一次性脚本；
   加「raw vs masked 身份字段不得相等」的自动扫描（这次是它抓住的唯一一条，正好证明这类扫描有效）。
2. **N-1**：`src/cli.rs:836-855` 整段删掉旧 `from_filter` 三写法，改成 `request.template` +
   `bindings` 的两种真实形状（`{"Filters":[{"Name":"zone","Values":["{zone}"]}]}` / 顶层标量）。
3. **N-3**：定一下「5xx + 业务信封」到底该不该重试（规格 vs 实现二选一，写进已知限制也行）。
4. **N-4**：给 `access-cvm-real.json` 加一条 `validate` 测试（ok + 5 字段 3/3）。
5. **N-5**：`TENCENTCLOUD_*` 兜底要么删（强制契约声明），要么写进「已知限制」并说明它不是分支。
