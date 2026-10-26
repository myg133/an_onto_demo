# QA 审核报告：REQ-152（阶段1 结构 + 阶段2 契约 + CP4）

> 审核人：QA Agent · 2026-10-26
> 被测：`feature-REQ-152`（分支 `feature/REQ-152`，HEAD `6d6badf`，5 条 commit：`8efd323` / `380510e` / `cb35e46` / `72a7755` / `6d6badf`）
> 审核标准：**「别人照它能不能接」** —— AC 逐条自己验，不接受「Dev 说过了」。
> 证据：worktree 内 `.local/qa152/`（独立 `CARGO_TARGET_DIR`：`.local/cargo-qa152`（HEAD）、`.local/cargo-qa152-base`（基线 `8efd323`）、`.local/cargo-qa152-c-<sha>`（逐 commit））
> 未改 feature 分支（`git status` 全程干净）、未 push、未碰 `Deploy/`、未做真调（无凭据）。

---

## 0 · 结论

**PASS（带非阻断）**。

| 面 | 判决 | 依据 |
| --- | --- | --- |
| 阶段1 结构：两个 bin / 镜像只一个 / 产品 bin 无执行器 | **PASS** | AC-006 五条判据全绿（自编自测） |
| 阶段2 契约：v1 不破、v2 给完整配方、只透传不推断 | **PASS** | 14 类 × 10 键齐全；v1 严格 4 键；`signed_headers` 删掉即消失 |
| CP4：`DUPLICATE_PLACEMENT` + `request.defaults` | **PASS** | 红/绿各一次；defaults 原样透出、源没写即整个键不出现 |
| R3 产品路径零厂商 token | **PASS** | 0 命中，变异后 1 命中（能红） |
| 服务面零回归（R1） | **PASS** | 8 case 与基线 `8efd323` **逐字节相同**（含两视图 TTL） |
| AC-110 `auth` 可实现规范 | **PASS** | QA 只读文档独立实现，复现全部 6 个中间值 + 签名 `10b1a37a…6c4f` |
| 全量测试 | **PASS（无新增红）** | 1771 / 183 / 3；183 条失败与基线**逐条同名**（178 缺环境 + 2 缺夹具 + 3 其他），零新增红 |

**非阻断项（建议本需求内补，不阻塞契约签字）**

| # | 项 | 影响面 |
| --- | --- | --- |
| N1 | **Deploy 侧未闭环**：`gen-serve-mapping.py`、`am-tencent.serve.json` 仍在；17 处旧 bin 名（`BUILD.md` / `README.md` 说「镜像里四个二进制」）未改 | 阶段1 的 AC-001 第 3 条 + 需求 §⑦ **不成立**；运维/客户照 `Deploy/README.md` 会以为镜像里有 4 个 bin |
| N2 | `docs/AUTH-SPEC.md`、`docs/RECIPE.md` 落位 `docs/`，而 AC-008 判据写的是 `skills/ontok-query/AUTH-SPEC.md`；`skills/ontok-query/SKILL.md` **一处都没提**这两篇 | 接入方加载的是 skill，文档在 skill 之外 ⇒ 发现路径断 |
| N3 | AUTH-SPEC §7.1 的 body 没写清 `\u672a\u547d\u540d` 是**字面 6 字符转义**还是真中文；两种读法只有一种对得上期望哈希 | 照文档实现的人第一次会算出 `1e07682a…`（≠ 文档 35e9c5b0…），得靠试错 |
| N4 | 400 `NOT_DB_SOURCE` 的 hint 仍只列举 4 个键（`get.class/filter/pushdown/ref`）；v2 下实为 10 键 | 提示文案比契约旧；因 AC-108 要求逐字节不变，**不许**贸然改 |
| N5 | `ontok-builder access fetch --help` 的「可以照抄的一整条」写的是 `ontok access fetch …`（服务面已无该子命令） | FDE 照抄 ⇒ `error: unrecognized subcommand 'access'`（Dev 已在注释里登记为取舍） |

---

## 1 · 两个 bin + AC-006 五条判据（全部自己跑）

独立构建：`CARGO_TARGET_DIR=.local/cargo-qa152` + `cargo build --release --bins` ⇒ **exit 0 / 2m18s**（`.local/qa152/build-release.log`）。

| # | 判据 | 实测 | 期望 | 判 |
| --- | --- | --- | --- | --- |
| ① | `target/release` 下 bin 数 | `ontok.exe` 9,280,000 B · `ontok-builder.exe` 17,398,272 B，**前缀匹配共 2 个** | 恰好 2 | ✅ |
| ② | 四个老 bin | `ontok-introspect` / `ontok-propose` / `ontok-translate-names` / `ontok-gate-preflight` **全部 False** | 0 个 | ✅ |
| ③ | `strings ontok \| grep -c 'X-TC-Action'` | **0**（按文件字节数 `X-TC-Action` 出现次数；同法测得 `ontok-builder` = **1**，证明判据抓得到） | 0 / >0 | ✅ |
| ④ | `ontok access fetch --allow-network` | `error: unrecognized subcommand 'access'`，**exit 2**；`ontok --help` 只有 `query` / `mcp-server` / `serve` | 不认识 | ✅ |
| ⑤ | Dockerfile 里 `COPY --from=build … /usr/local/bin/ontok` | **只有 1 条**（`COPY --from=build /src/target/release/ontok /usr/local/bin/ontok`）；有效指令里不再出现三个老 bin 名 | 只拷一个 | ✅（读 Dockerfile，未 build 镜像） |

补充（不是 AC，但是同一个结构事实）：`ontok.exe` 相比基线 `8efd323` 只涨 **+111,616 B（+1.2%）**，`ontok-builder.exe` +50,688 B —— 执行器没被服务面链接回来。

`ontok --help` 实测输出（服务面只有三条）：

```
Commands:
  query       ⭐ REQ-123 · **查询服务对外**（REST + 远程 MCP…）
  mcp-server  REQ-027 · 以**标准 MCP**（stdio）…
  serve       REQ-093 · 起一个**只读** HTTP 服务…
  help
```

`ontok-builder --help` 里四个工具都在（`introspect` / `propose` / `translate-names` / `gate-preflight`），且 `ontok-builder access fetch --dry-run-fixture` **实跑通过**（fixture 模式：扫 100 行 / 命中 33 / 返回 5，exit 0）⇒ R5「验收能力不丢」在离线侧成立（真调按指示不做：无凭据）。

---

## 2 · 契约 v1 / v2（AC-101 / 103 / 104 / 106 / 107）

方法：自己起 `ontok query serve --ontology-only --project cloud-resource --version 3 --access-mapping <映射>`，对 14 个 api 类逐个跑 `SELECT ?s WHERE { ?s a :<class> }`，从响应里抠 `@pending[0].get` 的**原始字节区间**（脚本 `.local/qa152/crawl_get.py`，不经过 Dev 的任何产物）。

### 2.1 同一份映射只改 `contract_version`（AC-103 的隔离实验）

| 映射 | `get` 键数 | 键 |
| --- | --- | --- |
| `Deploy/…/am-tencent.json`（**已签 v1**，只读） | **4** | `class` `filter` `pushdown` `ref` |
| 把 v2 文件里的 `contract_version` 改成 `1`（其余一字不动） | **4** | 同上 |
| `.local/am-v2/am-tencent.v2.json`（**已签 v2**，sha256 = `80fce221…e6db`，与 `.sha256` 一致） | **10** | 4 旧 + `source` `operation` `request` `iterator` `fields` `auth` |

> 两个 4 键映射的 **14 个响应 body sha256 全部相同（14/14）** ⇒ v1 分支与「不写版本」完全同形。
> `get.ref` 在 v1 下仍是 `access#CvmInstance@v1`（`@v1` 是**映射版本**，不是契约版本）——⛔ 别把它读成契约版本。

### 2.2 `get` 原文（v1 vs v2，同一个类 `cvm_instance`）

**v1（95 B）**

```json
{"class":"cvm_instance","filter":{},"pushdown":["region","zone"],"ref":"access#CvmInstance@v1"}
```

**v2（2461 B，`fields` 原样保留 —— 下面是响应里那一串原始字节）**

```json
{"class":"cvm_instance","filter":{},"pushdown":["region","zone"],"ref":"access#CvmInstance@v1","source":{"base_url":"https://cvm.tencentcloudapi.com","auth_mode":"tc3-hmac-sha256"},"operation":{"action":"DescribeInstances","content_type":"application/json","headers":{"Content-Type":"application/json; charset=utf-8","X-TC-Action":"{action}","X-TC-Region":"{region}","X-TC-Timestamp":"{timestamp}","X-TC-Version":"{version}"},"method":"POST","path":"/","vars":{"action":"DescribeInstances","version":"2017-03-12"},"version":"2017-03-12"},"request":{"bindings":{"region":{"source":"default"},"zone":{"source":"filter"}},"defaults":{"Region":"ap-guangzhou"},"paging":{"limit":"Limit","max_limit":100,"offset":"Offset"},"required":["region"],"template":{"Filters":[{"Name":"zone","Values":["{zone}"]}]}},"iterator":"$.Response.InstanceSet[*]","fields":{"cam_role_name":"CamRoleName","cpu":"CPU","…共 41 条，原样拍平…"},"auth":{"credential_env":["TENCENT_SECRET_ID","TENCENT_SECRET_KEY"],"signed_headers":["content-type","host","x-tc-action"],"timestamp":{"header":"X-TC-Timestamp","format":"unix-seconds"},"scope":["date","service","tc3_request"]}}
```

### 2.3 14 类键齐全度（AC-101）

`Y` = 键存在（v2 映射，服务面实测）：

| class | class | filter | pushdown | ref | source | operation | request | iterator | fields | auth |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| alarm_policy | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| cbs_disk | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| cbs_snapshot | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| cdb_instance | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| clb_listener | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| clb_load_balancer | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| cost_detail_bill | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| cvm_instance | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| db_backup | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| security_group | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| security_group_rule | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| ssl_certificate | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| subnet | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| vpc | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |

**14/14 类 10 键全在**；`operation` 里 `method`/`path`/`headers`/`vars` 都在。

### 2.4 `get` 体积（AC-107 ≤ 12 KB）

| class | v2 `get` 字节 | class | v2 `get` 字节 |
| --- | --- | --- | --- |
| alarm_policy | 1,554 | security_group | 1,420 |
| cbs_disk | 2,425 | security_group_rule | 1,560 |
| cbs_snapshot | 1,781 | ssl_certificate | **3,267（最大）** |
| cdb_instance | 2,469 | subnet | 1,690 |
| clb_listener | 1,878 | vpc | 1,622 |
| clb_load_balancer | 2,814 | db_backup | 1,697 |
| cost_detail_bill | 2,151 | cvm_instance | 2,461 |

- 最大 3,267 B（3.19 KB）⇒ 距 12,288 B **余量 73.4%**。
- v1 侧 14 类 `get` 只有 80–132 B。
- **上限是真的会咬人**（自己造肥映射验负向分支）：把 `ssl_certificate` 的 `fields` 加到 600 条 ⇒ 服务**拒绝启动**、exit 1，消息点名：

```
✗ access 映射 ".local\qa152\am-v2-fatfields.json" 不合法：有 1 个类的 `@pending.get` 配方超过 12288 B：
  ssl_certificate（14226 B）。通常是 `entities[].fields` 过肥 … ⛔ 本实现不会替你截断（截断等于悄悄改契约）。
```

### 2.5 `request.defaults` 逐类（CP4 核心 ①）

14 类**全部**有 `request.defaults`（该 AM 的 8 个源都写了 `defaults`）：

| class | `request.defaults`（原样） |
| --- | --- |
| **alarm_policy** | `{"Region":"ap-guangzhou","module":"monitor"}` ← **`module: "monitor"` 在**，大小写混排原样 |
| 其余 13 类 | `{"Region":"ap-guangzhou"}` |

负向（「源没写 defaults ⇒ 键不出现，⛔ 不塞 `{}`」）：把 `tencent-vpc` 源的 `defaults` 删掉后重跑服务面 ——

| 类 | `request` 里 `defaults` |
| --- | --- |
| vpc / subnet / security_group / security_group_rule（该源的 4 类） | **ABSENT（键不存在）** |
| 其余 10 类 | 仍为 `{"Region":"ap-guangzhou"}`（不受影响） |

> 是「键不存在」，不是 `"defaults":{}` —— 我用原始字节扫过 `"defaults"` 子串，4 类里 0 次出现。

### 2.6 AC-106 · `auth` 只透传、不推断

把 8 个源里的 `auth.signed_headers` **全部删掉**（其余不动）再跑服务面：

| | `get.auth` 的键 |
| --- | --- |
| 原 v2 映射 | `credential_env` · **`signed_headers`** · `timestamp` · `scope` |
| 删掉 `signed_headers` 后 | `credential_env` · `timestamp` · `scope`（**14/14 类都没有 `signed_headers`**） |

体积正好每类 −55 B（`,"signed_headers":["content-type","host","x-tc-action"]` 的长度），说明是**整键消失**而不是塞了个空数组。

### 2.7 AC-102 · 零凭据值

对 14 个 v2 响应全文跑 `AKID|SECRET_KEY\s*[:=]\s*"[^"]` ⇒ **0 命中**；`auth.credential_env` 恒为 `["TENCENT_SECRET_ID","TENCENT_SECRET_KEY"]`（只有变量名）。

### 2.8 AC-104 · 已签 v1 AM 一个字不改照常工作

`Deploy/k8s/overlays/uat-ops360/am-tencent.json` 只读加载 ⇒ serve 起得来（14 个 api 类），14 类 `get` 走 v1 分支（4 键），`description` 里的 `freshness` 对象形状被接受。**未修改该文件**（`Deploy/` 全程只读）。

---

## 3 · `DUPLICATE_PLACEMENT`（CP4 核心 ③）

做法：拿已签 v2 映射复制到 `.local/qa152/dupcase/`（**连 `fixtures/` 一起搬，否则相对夹具路径解析不了，会得到 14 条 `FIXTURE_UNREADABLE` 的假红**），只给 `cvm_instance` 的 `request.template` 加一条 `"Region": "{region}"`。

| 映射 | `ok` | errors | warnings | 其中 `DUPLICATE_PLACEMENT` | exit |
| --- | --- | --- | --- | --- | --- |
| 正常（已签 v2，夹具可解析） | true | 0 | 13 | **0** | **0** |
| 同一占位符既进 header 又进 body | true | 0 | 14 | **1** | **0** |

stderr 原文（点名称到 entity 与占位符）：

```
· DUPLICATE_PLACEMENT · entity CvmInstance：占位符 {region} 既进 header 又进 body，
  通常是错的（真调会报 UnknownParameter；离线体检原本查不出来）
```

⇒ **是 warning 不是 error，exit 仍 0**（`ok:true`），与 AC 一致。

**实现是否写死 `Region`**：

```
Select-String src/access/validate.rs -Pattern 'Region' -CaseSensitive  ⇒  0 行
```

读实现确认规则通用：`check_duplicate_placement()` 只做「`operation.headers` 各值里的占位符集合 ∩ `request.template` 里（递归下钻数组/嵌套对象）的占位符集合」，不认识任何具体 header / 参数名。单元测试 `duplicate_placement_is_a_warning_not_an_error` 绿。

---

## 4 · R3 · 产品路径零厂商 token（AC-002 / AC-105）

判据（AC-002 原文）：

```bash
grep -riE 'X-TC-|X-Amz-|X-Aliyun|tencent|aws|aliyun' \
     src/query/ src/access/{mod,validate,publish,path}.rs
```

| 被测 | 命中 |
| --- | --- |
| **HEAD `6d6badf`** | **0** ✅ |
| 基线 `8efd323`（阶段1） | 0 |
| `develop` `6682621`（阶段1 之前） | 1 条真命中（`publish.rs:314` 的 `"TENCENT_SECRET_ID"` 示例）+ 6 条**误报** |

**变异（证明判据能红）**：在 `src/query/source_gate.rs` 末尾加一行注释 `// QA-MUTATION X-TC-Action` ⇒ 命中 **1**（红）⇒ `git checkout --` 还原 ⇒ 命中 **0**（绿），`git status` 干净。

> ⚠️ 判据噪声（记一笔，不是缺陷）：`aws` 会误伤 `RawSpec` 这种标识符 ——  `6682621` 的 7 条里有 6 条是 `RawSpec`（→ `R-a-w-S`）。阶段1 里这个类型已改名为 `OntologyTokenSpec`，所以现在是真 0；但将来有人起个 `RawSpeculate` 之类的名字，这条 QG 会假红。

---

## 5 · 服务面零回归（AC-008 / AC-108 / R1）

**基线是哪份 commit**：我自己 `git archive 8efd323`（阶段1 那次结构提交，**R1 的比较基准就是它**）解到 `.local/qa152/base-8efd323/`，并按要求补了生成物（`out/` 343 个文件、`code/out/` 209 个文件，均来自工作区未被 git 跟踪的生成产物）；用 `.local/cargo-qa152-base` 独立编出 `ontok.exe`（exit 0 / 2m14s）。

> ⚠️ 我没有用 Dev 对拍时用的 `72a7755`：`8efd323` 到 HEAD 的范围**更大**（多覆盖 CP1/CP2/CP3/CP4 四条），更严。两种基线在「服务面等价」这个断言上应等价，我选更严的那个。

输入完全相同：`--ontology-only --project cloud-resource --version 3 --tokens <同一份假 token> --access-mapping Deploy/…/am-tencent.json`（v1 已签，**这样 pending 才该逐字节相同**）。端口不同、启动顺序相同。

| # | case | 请求 | 基线 `8efd323` status / bytes / sha256 | HEAD `6d6badf` status / bytes / sha256 |
| --- | --- | --- | --- | --- |
| 01 | health | `GET /api/health` | 200 / 195 / `5e2beb7d…c88a0` | 200 / 195 / `5e2beb7d…c88a0` |
| 02 | catalog | `GET /api/catalog?page=1&page_size=50` | 200 / 17,255 / `2f955632…7fa` | 200 / 17,255 / `2f955632…7fa` |
| 03 | describe | `GET /api/describe?name=:cvm_instance` | 200 / 19,557 / `529486eb…6c9` | 200 / 19,557 / `529486eb…6c9` |
| 04 | pending | `POST /api/sparql`（只碰 api 类） | 200 / 979 / `9172cbe4…76e` | 200 / 979 / `9172cbe4…76e` |
| 05 | rows 400 | `POST /api/sparql?format=rows` | 400 / 893 / `a925efa1…05f` | 400 / 893 / `a925efa1…05f` |
| 06 | 503 | `POST /api/sparql`（库源类 `:alarm_notice`） | 503 / 734 / `f9981631…c1c5` | 503 / 734 / `f9981631…c1c5` |
| 07 | TTL 客户视图 | `GET /api/ontology.ttl` | 200 / 209,429 / **`79741a7f…067a8b`** | 200 / 209,429 / **`79741a7f…067a8b`** |
| 08 | TTL 建模视图 | `GET /api/ontology.ttl?view=model` | 200 / 367,332 / **`e78e296e…6750d0`** | 200 / 367,332 / **`e78e296e…6750d0`** |

**8/8 逐字节相同；合并 sha 两边都是 `86d869490e22d3ea1f95cd3ede5d2cd50ab7d936614de6df27ec8f6738b09cab`。**
两视图 TTL 的 sha 也与 AC-009 里钉的 `79741a7f…` / `e78e296e…` 一致。

`04-pending` 的 body（证明 v1 下 `get` 只有 4 键、`meta.complete:false` 未变）：

```json
{"@context":{"ontok":"https://ontok.local/cloud_resource#","xsd":"…"},
 "@graph":[],
 "@pending":[{"@type":"ontok:cvm_instance","ontok:sourceKind":"api","reason":"source_is_api",
              "get":{"class":"cvm_instance","filter":{},"pushdown":["region","zone"],"ref":"access#CvmInstance@v1"}}],
 "summary":{"nodes":0,"pending":1,"row_count":0},
 "meta":{…,"complete":false,"unresolved_classes":["cvm_instance"],"mapping_version":1}}
```

> 说明：验「v2 加法」时用的是 v2 映射 —— 那时 `04-pending` 变成 3,345 B（多出 5 个配方段），这是**预期的加法**，不属于回归；回归对拍必须在同一份 v1 映射下做。

---

## 6 · AC-110 · `auth` 可实现规范（人判，**本需求最重要**）

做法：**只读 `docs/AUTH-SPEC.md`，不看 `src/access/tc3.rs`**，用 Python 从 §2–§6 自己实现（`.local/qa152/ac110_independent.py`），输入取 §7.1，期望值取 §7.2。

### 6.1 我实现的规范化串（207 字节，`\n` 显示为 `\n`）

```
POST\n
/\n
\n
content-type:application/json; charset=utf-8\n
host:cvm.tencentcloudapi.com\n
x-tc-action:describeinstances\n
\n
content-type;host;x-tc-action\n
35e9c5b0e3ae67532d3c9f17ead6c90222632e5b1ff7f6e89887f1398934f064
```

### 6.2 待签名字符串

```
TC3-HMAC-SHA256
1551113065
2019-02-25/cvm/tc3_request
7019a55be8395899b900fb5564e4200d984910f34794a27cb3fb7d10ff6a1e84
```

### 6.3 逐步对拍（左=我的输出，右=文档 §7.2）

| 中间值 | 我的输出 | 文档 | 判 |
| --- | --- | --- | --- |
| `HashedRequestPayload` | `35e9c5b0…8934f064` | `35e9c5b0…8934f064` | = |
| `SecretDate` | `da98fb70…6bd02be0` | `da98fb70…6bd02be0` | = |
| `SecretService` | `8d70cbef…5bd2752f` | `8d70cbef…5bd2752f` | = |
| `SecretSigning` | `b596b923…ed05f5af` | `b596b923…ed05f5af` | = |
| `HashedCanonicalRequest` | `7019a55b…ff6a1e84` | `7019a55b…ff6a1e84` | = |
| **`Signature`** | **`10b1a37a7301a02ca19a647ad722d5e43b4b3cff309d421d85b46093f6ab6c4f`** | 同 | **=** |

`Authorization`：

```
TC3-HMAC-SHA256 Credential=AKID********************************/2019-02-25/cvm/tc3_request,
SignedHeaders=content-type;host;x-tc-action,
Signature=10b1a37a7301a02ca19a647ad722d5e43b4b3cff309d421d85b46093f6ab6c4f
```

**结论：照文档实现得出来** —— §0 的六步、§3.1 的五行规则、§3.2 的「规范 header 串自带 \n、后面还要再加一个 \n」、§4 的 scope 拼法、§5 的 `"TC3"+SecretKey` 派生、§6 的 Authorization 拼法，全部可落地，一步不缺。

### 6.4 我踩到的两处（⇒ N3，非阻断但建议改文档）

1. **§3.2 的骨架图容易多写一个 `\n`**：我第一版在 `query` 后写了两个换行，`HashedCanonicalRequest` 变成 `c55528a3…`。是 §7.2 的**规范串原文**（`x-tc-action` 行后面只有一个空行）+ 期望哈希把我拽回来的 —— 也就是说 **§7 的自测向量是这篇文档的保险丝**，不能删。
2. **§7.1 的 body 有歧义**：`{"Values": ["\u672a\u547d\u540d"]…}` 在 Markdown 表格里看不出是「字面反斜杠 + u（6 字符）」还是「真中文 `未命名`」。两种读法我各算了一遍：

   | body 读法 | `HashedRequestPayload` | 是否等于文档 |
   | --- | --- | --- |
   | 字面转义序列（ASCII，含 `\u672a`） | `35e9c5b0…` | ✅ |
   | 真中文 UTF-8（`未命名` 三字） | `1e07682a…` | ❌ |

   建议文档补一句：「body 是**字面 ASCII** 的 `\u672a\u547d\u540d`（反斜杠 + u 共 6 字符 ×3），不是真中文」。

---

## 7 · `docs/RECIPE.md` 的可执行性（人判）

**只读它，三问答**：

**① 占位符从哪来？** —— 五类，合成**一张值表**，header 模板与 body 模板**都从它取**：
1. `operation.vars`（`{action}` / `{version}`）；
2. **客户端自己生成**的 `{timestamp}`（放 `auth.timestamp.header`，格式按 `auth.timestamp.format`）；
3. 调用方给的过滤值（`bindings.<名>.source = "filter"`，如 `{region}` / `{zone}`）；
4. `request.defaults`（`bindings.<名>.source = "default"`；键名大小写可能与占位符不同，**先精确、再大小写不敏感**，输出必须用映射里的原键名）；
5. 分页器（`{offset}` / `{limit}`，**不是**模板占位符，见 §四）。

三条硬规则也写得下得去手：default 只能来自 `request.defaults`、`required` 里没值就不许发。

**② 剪枝四条规则？** —— §三的表，按位置分四种：
1. 字符串里的占位符没值 ⇒ **剪掉该值**（不发 `""`）；
2. **数组元素（对象）**少一个键 ⇒ **整个元素剪掉**（`Filters:[{Name:"vpc-id",Values:[]}]` 会被拒）；
3. **顶层对象**少一个键 ⇒ **只丢那个键**（`{Module, PolicyName}` 缺 `PolicyName` 必须保住 `Module`）；
4. 数组被剪空 ⇒ **整个数组剪掉**。

外加三条补充：占位符必须独占整个字符串（`"prefix-{x}"` 直接拒绝）、顶层 `{}` 合法（= 不带过滤 ⇒ 全量分页取，别读成「没查到」）、剪枝只发生在模板上（`defaults`/`vars`/`required` 不剪）。

**③ `paging.value_type` 影响什么？** —— 影响**页位置值在 JSON 里的类型**：`"number"`（缺省）⇒ `{"Offset":0,"Limit":100}`；`"string"` ⇒ `{"Offset":"0","Limit":"100"}`。文档点名了「某些接口把 `Offset`/`Limit` 定义成 String，发数字会被拒（`InvalidParameter: input type should be string`），VPC / 子网 / 安全组属于这类」，并说**别去怀疑签名，先看 `paging.value_type`**。

**判定：三问全部答得出，且答案能直接落成代码** ⇒ 文档达标。附带的可执行细节也齐：`page` 1 基 / `offset` 0 基、`offset_base`、翻页终止（本页 < limit 或空页；某些接口会返空页 ⇒ 连续两页空再停）、`iterator` 的 `$.A.B[*]` 形状、`fields` 是拍平字符串且**不含** `identity.key_path`、缺路径不要丢整行。

---

## 8 · 全量测试（三列 + 逐条归因）

命令（两棵源码树各一次，**环境里无任何 `ONTOK_*`**，已确认 `ONTOK_* left: 0`）：

```
cargo test --all-targets --no-fail-fast
```

| | passed | failed | ignored | 汇总行数 |
| --- | --- | --- | --- | --- |
| **HEAD `6d6badf`** | **1771** | **183** | **3** | 108 |
| **基线 `8efd323`（我自己实测）** | **1759** | **183** | **3** | 106 |

**关键：183 条失败逐条同名**（用 `---- <name> stdout ----` 抽名字做集合差）：

```
HEAD 失败集合 183 条 · BASE 183 条
HEAD 多出来的（= 本次引入的新红）: 无
BASE 有而 HEAD 没有的（= 本次修好的）: 无
```

⇒ **本次改动零新增红**。

归因（183 条）：

| 类别 | 条数 | 依据 | 是否本次引入 |
| --- | --- | --- | --- |
| **缺环境**（无 `ONTOK_TEST_DATABASE_URL`） | **178** | panic 文案恒为 `缺 ONTOK_TEST_DATABASE_URL —— 口令不得入库，请导出环境变量: MissingEnv(...)`（`tests/common/mod.rs:233`、`src/db/apply.rs:225`、`src/extract/introspect.rs:778` 等） | ❌ 基线同红 |
| **缺夹具** | **2** | `real_schema_drop_rate_report` / `test_real_schema_customer_id_has_exactly_one_display_name`：`读不到脱敏 schema.json：../.review-workspace/fix-props/schema.json（os error 3）` | ❌ 基线同红 |
| **其他（历史遗留断言）** | **3** | `test_cli_end_to_end_output_is_byte_identical_to_legacy_extract`（items 断言不等）、`test_req084_every_registered_name_is_still_literally_at_its_line`（`propose.rs:280` 位置对不上）、`test_req084_no_unregistered_real_name_remains_in_the_registered_files`（`concept.rs` 9 处 vs 登记 5 处） | ❌ 基线同红（三条在 `8efd323` 上一模一样地红） |
| 代码问题 | **0** | —— | —— |

`ignored` 3 条（两棵树相同）：`child_writes_twenty_audit_lines` / `req149_shape_evidence_with_derived_real_mapping` / `todo_identifier_defenses_have_own_assertions`。

**新增用例全绿（17 条，逐条实测）**

| 用例 | 结果 |
| --- | --- |
| `cargo_declares_exactly_two_bins` | ok |
| `service_entry_does_not_reference_the_builder_cli` | ok |
| `builder_entry_merges_the_four_tools_as_subcommands` | ok |
| `dockerfile_copies_only_ontok` | ok |
| `the_four_tool_sources_are_still_modules_of_the_builder` | ok |
| `contract_v1_has_only_the_four_legacy_keys` | ok |
| `contract_v2_emits_the_whole_recipe_verbatim` | ok |
| `missing_auth_declarations_are_absent_not_inferred` | ok |
| `an_oversized_recipe_is_a_named_load_error` | ok |
| `source_defaults_are_passed_through_verbatim` | ok |
| `a_source_without_defaults_has_no_defaults_key` | ok |
| `the_documented_vector_recomputes_byte_for_byte` | ok |
| `the_spec_document_carries_the_same_vector` | ok |
| `auth_declarations_are_optional_and_never_inferred` | ok |
| `contract_v2_is_accepted_and_remembered` | ok |
| `operation_headers_and_vars_are_data` | ok |
| `duplicate_placement_is_a_warning_not_an_error` | ok |

> 没有删测试：抽查 `empty_iterator_is_an_error_not_a_vacuous_pass`、`sha256_matches_fips_vectors`、`a_token_in_the_query_string_does_not_work`、`ttl_rejects_unknown_views_and_parameters` 等仍在源码里、仍在跑并通过（它们在基线日志里「消失」是串行审计行插进 `test … ok` 同一行造成的解析噪声，不是被删）。
> 依赖未增：`git diff 6682621..HEAD -- Cargo.toml Cargo.lock` ⇒ **`Cargo.lock` 0 行变化**，`Cargo.toml` 只多 15 行（`autobins = false` + 第二个 `[[bin]]` + 注释），零新 crate。

---

## 9 · 「崩溃后还剩多少可用产物」——5 条 commit 的独立可编译性（P0 #3 的延伸）

上一次（阶段2 第一次派单）崩在**705 行未提交 / 32 个编译错误 / 零产物**。这次逐 commit 验：`git archive <sha>` → 独立 `CARGO_TARGET_DIR` → `cargo check --all-targets`。

| commit | 内容 | 改动规模 | `--all-targets` | 单文件最大改动 |
| --- | --- | --- | --- | --- |
| `8efd323` | 阶段1 结构：两个 bin / serve 吃全量 AM | 45 files, +1236/−512 | ✅（另编 release 成功） | `src/cli.rs` 436 行（多为搬运） |
| `380510e` | CP1 `operation` 泛化 + `auth` 三项声明 | 1 file, +295/−11 | ✅ exit 0 | `src/access/mod.rs` **306 行** |
| `cb35e46` | CP2 `get` 配方（6 段） | 4 files, +491/−30 | ✅ exit 0 | `src/query/source_gate.rs` **284 行** |
| `72a7755` | CP3 `AUTH-SPEC.md` + 文档/实现同源对拍 | 2 files, +402 | ✅ exit 0 | `docs/AUTH-SPEC.md` 271 行（文档） |
| `6d6badf` | CP4 `defaults` + `DUPLICATE_PLACEMENT` + `RECIPE.md` | 6 files, +393/−11 | ✅（全量测试已跑） | `docs/RECIPE.md` 223 行（文档） |

**判**：5/5 可独立编译，任何一个 commit 之后崩，都留下**可编译、可跑测试的树**；`AUTH-SPEC.md` / `RECIPE.md` 各自是**独立 commit**，即使 CP4 崩了文档也已落库。这比上次「705 行未提交 / 32 错 / 零产物」是**结构性改善**，不是行数游戏。

> ⚠️ 给下一个 QA 的坑（我自己踩了）：**不要给不同 `git archive` 出来的源码树共用同一个 `CARGO_TARGET_DIR`**。我第一次共用时，`cb35e46` / `72a7755` 报 `error[E0609]: no field recipe on type GetInstruction` —— 那是复用了 `380510e` 的旧 rlib 造成的假红；每个 commit 用独立 target dir 后 **3/3 全绿**。

---

## 10 · 问题清单（分级）

| 级 | # | 问题 | 证据 / 影响 | 建议 |
| --- | --- | --- | --- | --- |
| **必修（阶段1 AC 未闭环）** | N1a | `Deploy/k8s/overlays/uat-ops360/gen-serve-mapping.py`、`am-tencent.serve.json` **仍在** | `Test-Path` 均为 True；AC-001 第 3 条要求「已删除」 | 在 Deploy 仓删掉（一次 schema 已统一，投影脚本已无存在理由） |
| **必修（阶段1 AC 未闭环）** | N1b | Deploy 文档 **17 处**旧 bin 名未改（需求 §⑦ 明列：`BUILD.md` 6 / `PROJECT-ONBOARDING.md` 4 / `README.md` 3 / `UAT-PREREQ.md` 2 / `PRODUCTION-READINESS.md` 2） | `BUILD.md:19`、`README.md:21` 明说「镜像里四个二进制」——**与 AC-006 ⑤ 直接矛盾**；`UAT-PREREQ.md:86-87` 让 UAT 执行 `ontok-introspect` / `ontok-propose`（已不存在） | 同步改成 `ontok` / `ontok-builder <子命令>`；`README` 改成「一个 bin」 |
| 建议（不阻塞） | N2 | `AUTH-SPEC.md` / `RECIPE.md` 在 `docs/`，AC-008 写的是 `skills/ontok-query/AUTH-SPEC.md`；且 `skills/ontok-query/SKILL.md` 一处都没引用这两篇 | 接入方加载的是 skill；文档在 skill 视野之外 | ①把 AC-008 的路径口径改成 `docs/`（AC-110 已是这个口径）；②在 `skills/ontok-query/SKILL.md` 里加两行指针 |
| 建议 | N3 | AUTH-SPEC §7.1 body 的 `\u672a\u547d\u540d` 有歧义 | 见 §6.4：真中文读法得 `1e07682a…` ≠ 文档 `35e9c5b0…` | 补一句「字面 ASCII 转义序列」 |
| 建议 | N4 | 400 `NOT_DB_SOURCE` 的 hint 只列 4 键 | 与基线逐字节相同（AC-108 要求），但 v2 下 `get` 是 10 键 | 下个**允许改文案**的版本里补；本期不动 |
| 建议 | N5 | `ontok-builder access fetch --help` 的示例写成 `ontok access fetch …` | 照抄 ⇒ `unrecognized subcommand` | Dev 已在注释里登记取舍（`gate-preflight` 的帮助正文被「文案里的 flag 必须真被解析器收下」的判据逐词钉着）；建议后续把示例改成 `ontok-builder …` |
| 记录 | N6 | QG 的 `aws` 会误伤 `RawSpec` 这类标识符 | `develop` 7 条里 6 条是 `RawSpec` | 判据可考虑改成词边界（`\baws\b`），或维持并接受偶发假红 |

---

## 11 · 我认为**最可能被外部接入方踩到**的一点

**「`get` 里键不存在」这件事，客户端无法区分是「映射没升 v2」还是「这个类就没这项声明」——而响应里没有任何地方告诉你契约版本。**

- 实测：`@pending[].get` 的 10 个键里**没有** `contract_version`；`meta` 里只有 `mapping_version`（那是**映射**版本，`get.ref` 的 `@v1` 用的是它），**不是契约版本**。
- 后果：Deploy 那份**已签 v1** 映射（`contract_version: 1`）下，14 类 `get` 都是 4 键 —— 这是 R2 的**正确行为**，但接入方看到的就是「平台没给配方」，于是回到「自己猜接口 / 自己造签名」的老路，这正是本需求要消灭的现象。
- RECIPE 的兜底是「**见到 4 键就说明这份映射还没升到 v2**」；可 4 键 vs 部分配方（比如某个类只有 `source`+`iterator` 而 `auth` 缺 `signed_headers`）在客户端看来是同一类现象：**只能靠数键猜**。而 AC-106 明确要求「缺就是不声明」，于是「缺」同时承载了三种语义：版本没升 / 这一类没配 / 作者故意不给。

**建议（成本很低，收益直接）**：在 `@pending[].get` 或 `meta` 里加一个 `contract_version`（加法式，v1 客户端忽略即可），或在 `get` 缺席时给一句 `hint` 说明「本映射契约版本为 1，未声明取数配方，请向平台索取 v2 映射」。这样接入方第一次接触就能自己判断该不该去要新映射，而不是凭键数猜。

---

## 12 · 证据清单（`.local/qa152/`）

| 文件/目录 | 内容 |
| --- | --- |
| `build-release.log` | 独立 target dir 的 release 构建（exit 0） |
| `base-8efd323/` + `base-build.log` + `.local/cargo-qa152-base` | 自编基线（含 `out/` 343 + `code/out/` 209 生成物） |
| `c-<sha>/` × 3 + `.local/cargo-qa152-c-<sha>` | 三个中间 commit 的独立编译验证 |
| `crawl_get.py` + `v2/` `v1/` `bumponly/` `nosig/` `nodef/` | 5 次服务面抓取：v2 / v1 已签 / 只改版本号 / 删 `signed_headers` / 删源 `defaults` |
| `serve_compare.py` + `serve-base/` + `serve-head/` | 8 case 逐字节对拍（含 `sha256.tsv`、`combined.sha`） |
| `validate/` + `dupcase/` | `access validate` 正常/重复占位两跑（含 `fixtures/`） |
| `am-v2-fatfields.json` | 12 KB 上限的负向验证 |
| `ac110_independent.py` + `ac110.out.txt` | AC-110 独立实现与对拍输出 |
| `test-head.log` / `test-base.log` + `parse_tests.py` + `parse-*.txt` + `diff_fails.py` | 全量测试三列与逐条归因 |
| `fetch-dry.json` / `fetch-dry.err.txt` | `ontok-builder access fetch --dry-run-fixture` 实跑（R5 离线侧） |
| `tokens.json` | 自造假 token（`QA152-TOKEN-0001-NOT-REAL`），不进任何真实凭据 |
