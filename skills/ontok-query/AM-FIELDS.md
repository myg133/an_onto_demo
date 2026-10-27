# AM-FIELDS · access mapping 的每个键是什么，`access_fetch.py` 用到哪些

> 读者：**要改 AM 的人**（映射作者 / 运维）和**要用脚本取数的人**（数字员工）。
> 一句话：AM 有 30 多个键，取数只用其中一部分。这份文档逐键说清「是什么 / 脚本用不用 / 怎么用」，
> 这样改 AM 时不会去动那些脚本根本不读的字段，也不会以为「我写了脚本就会用」。
>
> 配套：`AUTH-SPEC.md`（签名算法）、`RECIPE.md`（`@pending[].get` 怎么用）、`SKILL.md` §4.5（怎么跑脚本）。

图例：✅ 脚本用；◐ 脚本用但**缺了有默认值/只影响某一模式**；⛔ 脚本**完全不读**（写了不生效，但通常对别的工具仍有意义）。

---

## 一 · 顶层（7 个键）

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `contract_version` | AM 契约版本。**只有 `1` 是本脚本认识的** | ✅ 不认识就 `am.invalid`（退出 2），⛔ 不做「尽力兼容」 |
| `mapping_version` | 这份映射的版本号（`@pending.get.ref` 里的 `@v<版本>`） | ✅ 原样抄进输出 `meta.mapping_version` |
| `project` | 生成这份 AM 的项目标记（如 `"extracted"`） | ⛔ |
| `ontology_version` | 对应本体版本 | ⛔（`--check` 只打印，不判断） |
| `sources` | 数据源列表（**8 个**：cvm / vpc / cbs / clb / cdb / ssl / monitor / billing） | ✅ |
| `entities` | 可取的实体列表（**14 个**） | ✅ |
| `relations` | 实体间关系（跨源边） | ⛔ 取数不需要边；跨源拼装由上层做（`SKILL.md` §4.3 的「跨源边」） |

> `relations` 现在是空数组。脚本不读它，**也不因为它是空就报错**——取数只关心「一个 entity → 一次接口」。

---

## 二 · `sources[]`（每个数据源）

```json
{ "id": "tencent-cvm", "kind": "http-json",
  "base_url": "https://cvm.tencentcloudapi.com",
  "auth": { "mode": "tc3-hmac-sha256",
            "credential_env": ["TENCENT_SECRET_ID", "TENCENT_SECRET_KEY"] },
  "defaults": { "Region": "ap-guangzhou" } }
```

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `id` | 源标识；entity 的 `source` 指向它 | ✅ 找不到 ⇒ `am.invalid` 并列出已有 id |
| `kind` | 源类型。**v1 只做 `http-json`** | ✅ 别的（`mysql` 等）⇒ `am.invalid`（demand §六：不扩源类型） |
| `base_url` | 接口根地址；**`host` 从它来**（含非默认端口） | ✅ 拼 URL + 进签名规范头 `host:` |
| `auth.mode` | 鉴权算法。**只认 `tc3-hmac-sha256`** | ✅ 别的 ⇒ `am.invalid` |
| `auth.credential_env` | **环境变量名**，`[0]`=SecretId、`[1]`=SecretKey。⛔ 永远不放值 | ✅ 只从这两个变量读；缺 ⇒ `credential.missing`（退出 3），消息点名变量、⛔ 不回显值 |
| `auth.signed_headers` | 哪些 header 参与签名（**小写**） | ◐ 缺 ⇒ 用 TC3 默认 `content-type;host;x-tc-action`（`--check` 会明说） |
| `auth.timestamp.header` / `.format` | 时间戳放哪个头、什么格式 | ◐ 缺 ⇒ `X-TC-Timestamp` / `unix-seconds` |
| `auth.scope` | scope 由哪几段拼（段名顺序即拼接顺序） | ◐ 缺 ⇒ `["date","service","tc3_request"]` |
| `auth.service` | 产品名（如 `cvm`），进待签名的 scope | ◐ 缺 ⇒ 取 `base_url` 主机名第一段（`cvm.tencentcloudapi.com` → `cvm`） |
| `defaults` | 默认值表（契约 v1 放这里，v2 在 `request.defaults`） | ✅ `bindings.<名>.source = "default"` 时从这里取值；键名大小写可与占位符不同（先精确、再大小写不敏感） |

⚠️ **`auth` 段当前只声明了 `mode` + `credential_env`**。缺的那四项（`signed_headers` / `timestamp` / `scope` / `service`）脚本按 TC3 标准形状兜底，并且 **`--check` 每次都会打一条提示**——不是静默猜。要精确控制就补进 AM（`AM-FIELDS` 这一节就是补法的清单）。

---

## 三 · `entities[]`（每个可取的实体）

14 个 entity 的公共形状（以 `CvmInstance` 为例，节选）：

```json
{ "id": "CvmInstance", "class": "cvm_instance", "source": "tencent-cvm",
  "reviewed_by": "michael@2026-10-26",
  "operation": { "action": "DescribeInstances", "version": "2017-03-12",
                 "method": "POST", "content_type": "application/json" },
  "iterator": "$.Response.InstanceSet[*]",
  "identity": { "key_path": "InstanceId" },
  "iri_template": "ontok:cvm_instance/{id}",
  "fields": { "instance_id": { "path": "InstanceId" }, "cpu": { "path": "CPU" } },
  "request": { "required": ["region"],
               "template": { "Region": "{region}",
                             "Filters": [{ "Name": "zone", "Values": ["{zone}"] }] },
               "bindings": { "region": { "source": "default" },
                             "zone": { "source": "filter" } },
               "paging": { "offset": "Offset", "limit": "Limit", "max_limit": 100 } },
  "paging": { "offset_param": "Offset", "limit_param": "Limit",
              "max_limit": 100, "total_path": "$.Response.TotalCount" },
  "capability": { "pushdown": ["region", "zone"],
                  "local_only": ["instance_state", "cpu", "memory"],
                  "max_scan_rows": 5000, "cost_hint": "free" },
  "freshness": { "mode": "realtime", "note": "查询型接口…" },
  "fixture": "fixtures/cvm-DescribeInstances.real.json" }
```

### 3.1 身份与来源

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `id` | entity 标识（如 `CvmInstance`）；`--entity` 可直接用它 | ✅ |
| `class` | 本体 class（如 `cvm_instance`）；`--entity` 也接受它 | ✅ 进 `@type` = `ontok:<class>` |
| `source` | 指向 `sources[].id` | ✅ 不存在 ⇒ `am.invalid` |
| `reviewed_by` | **谁签的这份映射**（`name@YYYY-MM-DD`） | ✅ **门禁**：缺失/空串 ⇒ `am.unsigned`（退出 2），未签**一律不许取数** |

> 未签取不了，这不是「配置没填完」的软提示，而是**硬门禁**：`reviewed_by` 为空时脚本连一次请求都不发。

### 3.2 `operation`：调哪个接口

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `action` | 接口名（`DescribeInstances`） | ✅ 进 `X-TC-Action` 头；规范签名里**转小写** |
| `version` | 接口版本（`2017-03-12`） | ✅ 进 `X-TC-Version` 头 |
| `method` | HTTP 方法 | ◐ **只实现 POST**（别的 ⇒ `am.invalid`；AM 里现在全是 POST） |
| `content_type` | 请求体类型 | ◐ 缺 ⇒ `application/json; charset=utf-8`；**签名用的就是发出去的那个值** |
| `path` | 请求路径 | ◐ 缺 ⇒ `/`（API 3.0 恒 `/`） |
| `headers` | header 模板（`{action}`/`{version}`/`{timestamp}`/`{region}` …） | ◐ **声明了就用它**；没声明才补 TC3 标准头（见 §五） |
| `vars` | 填 header 模板的值表 | ◐ 本 AM 没有 `headers`，所以也用不上；写了脚本会拿去填 |

### 3.3 `request`：请求体怎么拼

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `template` | 请求体模板；`{name}` 是占位符 | ✅ 按 RECIPE §三 剪枝后发出（缺值的过滤器整条剪掉；顶层只丢那个键） |
| `required` | **绑定名**列表：没值就不许发请求 | ✅ 提前拦住并点名缺哪个（对应 `--param`） |
| `bindings` | 占位符的值从哪来：`default` / `filter` / `paging` | ✅ 三类都实现；`shape: array` + `value_type: integer` 也照办 |
| `paging` | 请求体里的分页参数名与类型 | ✅ 读 `offset` / `page` / `limit` / `max_limit` / `value_type` / `offset_base` |
| `defaults` | 契约 v2 的默认值（与 `sources[].defaults` 并存） | ◐ 两份都读，`request.defaults` 优先 |

**占位符值表**（RECIPE §二，脚本照此实现）：

| 占位符 | 从哪来 |
| --- | --- |
| `{action}` / `{version}` | `operation.vars` |
| `{timestamp}` | 脚本生成（`auth.timestamp.format` 决定格式） |
| `source = filter` 的名字（`{zone}` / `{vpc_id}`…） | `--param` 给的过滤值 |
| `source = default` 的名字（`{region}` / `{module}`…） | `sources[].defaults` → `request.defaults` |
| `{offset}` / `{limit}` | 分页器（不是模板占位符，见 §3.5） |

**两条脚本额外遵守的规则**：

1. **`--param` 覆盖同名默认值**（显式 > 默认）。否则 `region` 永远只能取 `ap-guangzhou`，`pushdown` 里的 `region` 就没意义了。
2. **逗号只在 `shape = "array"` 时才当分隔符**。`SslCertificate` 的 `status` 声明了 `shape: array`，所以 `--param status=1,2,3` 会变 `[1,2,3]`；而 `CdbInstance` 的 `instance_ids` **没声明 shape**，`--param instance_ids=a,b` 会整串当一个值（见 §六「待办」）。

### 3.4 `iterator` / `fields` / `identity` / `iri_template`：怎么切行、怎么映射

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `iterator` | 响应里「一行实体」的路径 | ✅ **JSONPath 子集**：只支持 `$.a.b[*]` 与 `$.a.b`（`$.a.b.c[*]` 也可以）。数组下标 `[0]`、`[*]` 在中间、`..`、过滤器一律 `am.invalid` 并说明改法 |
| `identity.key_path` | 主键在一行里的位置 | ✅ 取出来填 `iri_template` 的 `{id}`；没有 identity 的行会被跳过并记 `meta.skipped_no_identity`（不是静默丢） |
| `iri_template` | `@id` 模板，必须含 `{id}` | ✅ 缺失 `{id}` ⇒ `am.invalid` |
| `fields.<属性>.path` | 属性 ← 一行里的点号路径 | ✅ 逐层下钻；某行缺这条路径 ⇒ 该属性没有值，**不丢整行**（RECIPE §六） |
| `fields.<属性>.type` | 属性类型（`xsd:integer` 等） | ⛔ 脚本原样透传 JSON 值，不做类型转换 |

### 3.5 `paging`（实体级）与 `request.paging`（请求级）

AM 里**两份并存**，各管一件事：

| 键 | 在哪 | 是什么 | 脚本 |
| --- | --- | --- | --- |
| `offset_param` | `entities[].paging` | 偏移/页码参数名（`Offset` / `PageNumber`） | ✅ 决定 body 里哪个键吃偏移 |
| `limit_param` | `entities[].paging` | 每页条数参数名（`Limit` / `PageSize`） | ✅ |
| `max_limit` | 两处都有 | 服务端单页上限 | ✅ 两处都读；`--limit` 会被它夹住（`min(--limit, max_limit)`） |
| `total_path` | `entities[].paging` | 响应里总数的路径（`$.Response.TotalCount`） | ✅ **有它就按它判「取全」**；没有就按「短页/空页」判 |
| `offset` / `page` | `request.paging` | 同上两个名字（v2 写法） | ✅ 与实体级互为兜底 |
| `value_type` | `request.paging` | `number` / `string` | ✅ **实测必须看**：VPC / 子网 / 安全组的 `Offset`/`Limit` 要字符串，发数字会被拒（RECIPE §四） |
| `offset_base` | `request.paging` | `0` / `1`；页码从 1 起时写 `1` | ✅ 缺省：有 `page` ⇒ 1，否则 0 |

翻页终止（脚本实现，与 RECIPE §四一致但更严）：

| 情况 | 处理 |
| --- | --- |
| 有 `total_path` 且已取到 `TotalCount` | 取满就停（**精确**） |
| 没有总数 | 本页条数 < 本页 `limit` 就停；空页也停 |
| 到 `--max-rows` | 停，**`truncated=true` + 退出 8** |
| 到 1000 页保险丝 | 停，**`truncated=true` + 退出 8** |

⛔ 没有「静默截断」这条路：只要少取了，`meta.truncated` 一定是 `true`，退出码一定是 `8`。

### 3.6 `capability`：能下推什么

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `pushdown` | **服务端能筛**的字段名 | ✅ `--param` 的**白名单**；给了白名单外的名字 ⇒ `param.not_pushdown`（退出 2）并列出可用参数 |
| `local_only` | 只能拉回来在本地筛的字段 | ⛔ 脚本**不筛**：它只负责取回原始行，筛选是调用方的事（否则「看起来过滤了、其实是全量」更危险） |
| `max_scan_rows` | 单次扫描的行数建议上限 | ⛔ 脚本不拿它当硬限制（硬限制是 `--max-rows`，默认 1000） |
| `cost_hint` | 计费提示（`free` / …） | ⛔ 只给人看 |

### 3.7 `freshness` / `fixture`

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `freshness.mode` / `.note` | 数据新鲜度 / 口径说明 | ⛔ 纯文档，脚本不读 |
| `fixture` | 一份**真实形状的响应**（离线夹具） | ◐ `--dry-run` 用它取数（**不发网络、不要凭据**）；`--check` 检查文件在不在（缺了只是 warning） |

> 夹具的**来源**必须看清：`cvm-DescribeInstances.real.json` 是真实响应脱敏，
> 其余 13 份是照语料合成的**自证夹具**（`.local/access-mapping/fixtures/README.md`）。
> `--dry-run` 通过只说明「映射与夹具自洽」，⛔ 不等于「真接口能取」。

---

## 四 · `relations`

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `relations[]` | 实体间关系（`from` / `to` / 证据等级…），用于跨源遍历 | ⛔ **完全不读** |

取数是「一个 entity → 一次接口」；跨源边（一个类在库、一个在 API）要拆两段走，
那段编排在调用方（`SKILL.md` §4.3 末尾），不在这个脚本里。

---

## 五 · 脚本自己补的东西（AM 没写时的默认值）

这些不是「猜」，是**写死在脚本里的 TC3 标准形状**，而且 `--check` 每次都会打一条提示说明正在用默认值。

| 缺什么 | 脚本用什么 | 为什么可以这样 |
| --- | --- | --- |
| `auth.signed_headers` | `content-type;host;x-tc-action` | 与 AUTH-SPEC §7 官方向量同一组头；少了 `x-tc-action` 反而签不过 |
| `auth.timestamp` | `X-TC-Timestamp` / `unix-seconds` | API 3.0 的唯一形态 |
| `auth.scope` | `date/service/tc3_request` | 同上 |
| `auth.service` | `base_url` 主机名第一段 | `cvm.tencentcloudapi.com` → `cvm`；8 个源全部对得上 |
| `operation.headers` | 补 `Content-Type` / `X-TC-Action` / `X-TC-Version` / `X-TC-Timestamp` | AM 只声明了 action/version，头是算法要求的 |
| `operation.path` | `/` | API 3.0 恒 `/` |

**⚠️ `Region` 的放置（唯一一处「脚本比 AM 保守」）**：
`Region` 已经在 `request.template` 里进 body 时，脚本**不会**再补 `X-TC-Region` 头——
同一个值同时进 header 与 body 会得到 `UnknownParameter`（RECIPE §七①，真调 11 个类全中）。
反过来，如果某个 entity 的 body 里没有 `Region` 而有 `region` 值，脚本才补 `X-TC-Region`。
改 AM 时要记住这条：**一个值只放一处**。

---

## 六 · 已知限制与 AM 作者待办

脚本**不偷偷砍需求**，做不到的都在这儿写明：

| # | 限制 | 影响 | 建议 |
| --- | --- | --- | --- |
| 1 | `kind` 只支持 `http-json` | 别的源类型直接拒绝 | 需要新源类型就先提需求（demand §六：v1 不做） |
| 2 | `auth.mode` 只实现 `tc3-hmac-sha256` | 别的算法拒绝 | 同上 |
| 3 | `iterator`/`total_path`/`fields.path` 只支持 JSONPath 子集 | 复杂路径（`$.a[0].b`、`$.a[*].b`、过滤）会被**拒绝**而不是猜 | AM 里把路径落到子集内；实在不行报给脚本作者扩语法 |
| 4 | `CdbInstance.instance_ids` 没声明 `shape: array` | `--param instance_ids=a,b` 会整串当一个 ID | 在 `bindings.instance_ids` 加 `"shape": "array"`（一行改动） |
| 5 | `Region` 进 body 的写法与 RECIPE §七① 的告警有张力 | 真调时可能 `UnknownParameter`（本轮无凭据，未实测） | 真调验证后，二选一：删 body 的 `Region`（脚本自动补头），或确认该接口接受 body `Region` |
| 6 | 不做自动重试/退避 | 限流/超时要调用方自己重试 | `SKILL.md` 里已写明；需要就提需求 |
| 7 | 不把结果推到服务端 | 行只在你手上 | 这是设计（demand §六），别指望 `@graph` 自动回填 |

---

## 七 · 30 秒自查（改完 AM 之后）

```bash
python access_fetch.py --mapping am.json --check                      # 结构 + 签名门禁
python access_fetch.py --mapping am.json --entity <id> --dry-run      # 用夹具走一遍
python access_fetch.py --mapping am.json --entity <id> --param ... --max-rows 5   # 真取一小页
```

`--check` 只看结构（不取数、不要凭据）；`--dry-run` 要夹具、不要凭据；真取数才要环境变量。
