# AM-FIELDS · access mapping 的每个键是什么，`scripts/access_fetch.py` 用到哪些

> 读者：**要改 AM 的人**（映射作者 / 运维）和**要用脚本取数的人**（数字员工）。
> 一句话：AM 有 30 多个键，取数只用其中一部分。这份文档逐键说清「是什么 / 脚本用不用 / 怎么用」，
> 这样改 AM 时不会去动那些脚本根本不读的字段，也不会以为「我写了脚本就会用」。
>
> 配套：`AUTH-SPEC.md`（签名算法规格）、`RECIPE.md`（`@pending[].get` 怎么用）、
> `SKILL.md` §4.5（怎么跑脚本）与 §4.6（AM 契约：可以依赖什么）。
>
> 本文里的 `<...>` 都是**占位符**：换成你这套部署里的真实名字（某份具体 AM 的样例见
> `fixtures/`，那是示例，**不是规格**）。

图例：✅ 脚本用；◐ 脚本用但**缺了有默认值/只影响某一模式**；⛔ 脚本**完全不读**（写了不生效，但通常对别的工具仍有意义）。

---

## 一 · 顶层（7 个键）

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `contract_version` | AM **契约形状**的版本。脚本认 `1` / `2`（差别见 §1.1） | ✅ **白名单**：不在名单 ⇒ `am.invalid`（退出 2）并**列出支持的版本**，⛔ 不做「尽力兼容」 |
| `mapping_version` | 这份映射的**内容**修订号（`@pending.get.ref` 里的 `@v<版本>`） | ✅ 原样抄进输出 `meta.mapping_version` |
| `project` | 生成这份 AM 的项目标记 | ⛔ |
| `ontology_version` | 对应本体版本 | ⛔（`--check` 只打印，不判断） |
| `sources` | 数据源列表（一份 AM 里有几个源，取决于这套部署） | ✅ |
| `entities` | 可取的实体列表（一份 AM 里有几个 entity，取决于这套部署） | ✅ |
| `relations` | 实体间关系（跨源边） | ⛔ 取数不需要边；跨源拼装由上层做（`SKILL.md` §4.3 的「跨源边」） |

> `relations` 常常是空数组。脚本不读它，**也不因为它是空就报错**——取数只关心「一个 entity → 一次接口」。

### 1.1 `contract_version`：脚本认哪两版，对它们分别怎么对待

| 版本 | 契约形状的差别 | 脚本对它的行为 |
| --- | --- | --- |
| `1`（原始） | `auth` 只声明 `mode` + `credential_env`；签名四件套（`signed_headers` / `timestamp` / `scope` / `content_type`）不在契约里，按**签名器默认值**理解 | ✅ 照读；四项缺了用**签名器默认值**（`--check` 末尾会明说在用什么默认值） |
| `2` | **唯一的契约增量**：`auth` 可**显式**声明 `signed_headers` / `timestamp` / `scope`（+ `content_type`） | ✅ 照读；显式写的值与签名器默认值必须签出**逐字节相同**的 `Authorization`（`--selftest` 用同一签名向量对拍 —— 否则就是「换个写法把签名签坏了」） |
| `3` 及以上 / 非整数 | — | ⛔ **一律拒绝**：`am.invalid`（退出 2）+ 消息**列出支持的版本**（`1, 2`）；⛔ 不「尽力解析」、不按最新版硬读 |

> **版本改的是「契约期望」，不是脚本的读取逻辑。**
> 两版下脚本读的键**完全相同**，所以 v1 / v2 之间**没有任何按版本的行为分支**；
> `--check` 每次都会打印「识别到哪一版 + 该版本下脚本实际读了哪些 `auth` 键」。
> 下面 §二 / §三 的逐键表**不按版本分栏**：一个键在哪个版本都是同一个意思
> （某份 v2 数据多写了 `operation.path` / `.headers` / `.vars`，那是**数据侧差异**，不是契约新增能力）。

---

## 二 · `sources[]`（每个数据源）

```json
{ "id": "<source id>", "kind": "http-json",
  "base_url": "https://<host>",
  "auth": { "mode": "tc3-hmac-sha256",
            "credential_env": ["<凭据 ID 的变量名>", "<凭据密钥的变量名>"],
            "service": "<该接口所属服务名>" },
  "defaults": { "<参数名>": "<默认值>" } }
```

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `id` | 源标识；entity 的 `source` 指向它 | ✅ 找不到 ⇒ `am.invalid` 并列出已有 id |
| `kind` | 源类型。**v1 只做 `http-json`** | ✅ 别的（`mysql` 等）⇒ `am.invalid`（不扩源类型） |
| `base_url` | 接口根地址；**`host` 从它来**（含非默认端口） | ✅ 拼 URL + 进签名规范头 `host:` |
| `auth.mode` | 鉴权算法 = **签名器的注册名**（`--list-signers` 查） | ✅ 没有对应签名器 ⇒ **`auth.unsupported`（退出 4）** 并列出已支持的 mode，⛔ **不 fallback 到别的算法** |
| `auth.credential_env` | **环境变量名**，`[0]`=凭据 ID、`[1]`=凭据密钥。⛔ 永远不放值 | ✅ 只从这两个变量读；缺 ⇒ `credential.missing`（退出 3），消息点名变量、⛔ 不回显值 |
| `auth.signed_headers` | 哪些 header 参与签名（**小写**） | ◐ 缺 ⇒ 用**签名器自带**的默认值（`tc3-hmac-sha256` 是 `content-type;host;x-tc-action`；`--check` 会明说） |
| `auth.timestamp.header` / `.format` | 时间戳放哪个头、什么格式 | ◐ 缺 ⇒ 签名器默认（`X-TC-Timestamp` / `unix-seconds`） |
| `auth.scope` | scope 由哪几段拼（段名顺序即拼接顺序） | ◐ 缺 ⇒ 签名器默认（`["date","service","tc3_request"]`） |
| `auth.service` | **该接口所属的服务名**，进待签名 scope 与派生密钥。**`tc3-hmac-sha256` 必填** | ✅ 缺/空 ⇒ `am.invalid`（退出 2）并**点名 `auth.service`**；⛔ **不再从 `base_url` 的 host 猜** |
| `defaults` | 默认值表。两处都读：`sources[].defaults` 是基础，`request.defaults`（若写了）覆盖它 —— **与 `contract_version` 无关** | ✅ `bindings.<名>.source = "default"` 时从这里取值；键名大小写可与占位符不同（先精确、再大小写不敏感） |

⚠️ **`auth.service` 是签名输入，不是装饰**：它和 `auth.signed_headers` / `timestamp` / `scope` 一起决定
`Authorization` 的字节。猜错**不会报错**，只会签出一个「看起来对」的错签名 —— 所以脚本要求它显式声明。
另外四项（`signed_headers` / `timestamp` / `scope` / `content_type`）缺了由**签名器**兜底，
`--check` 每次都会打一条提示说明正在用默认值（不是静默推断）。

---

## 三 · `entities[]`（每个可取的实体）

公共形状（节选，`<...>` 是占位符）：

```json
{ "id": "<entity id>", "class": "<class>", "source": "<source id>",
  "reviewed_by": "<签署人>@<YYYY-MM-DD>",
  "operation": { "action": "<接口名>", "version": "<接口版本>",
                 "method": "POST", "content_type": "application/json" },
  "iterator": "$.<字段>.<数组>[*]",
  "identity": { "key_path": "<记录里的键名>" },
  "iri_template": "ontok:<class>/{id}",
  "fields": { "<属性>": { "path": "<记录内的路径>" },
              "<另一个属性>": { "path": "<外层>.<内层>" } },
  "request": { "required": ["<绑定名>"],
               "template": { "<参数名>": "{<占位名>}",
                             "<数组参数名>": [{ "<键>": "{<占位名>}" }] },
               "bindings": { "<占位名>": { "source": "default" },
                             "<另一个占位名>": { "source": "filter" } },
               "paging": { "offset": "Offset", "limit": "Limit", "max_limit": 100 } },
  "paging": { "offset_param": "Offset", "limit_param": "Limit",
              "max_limit": 100, "total_path": "$.<字段>.<总数>" },
  "capability": { "pushdown": ["<可下推参数>"],
                  "local_only": ["<只能本地筛的属性>"],
                  "max_scan_rows": 5000, "cost_hint": "free" },
  "freshness": { "mode": "realtime", "note": "<口径说明>" },
  "fixture": "fixtures/<响应夹具>.json" }
```

### 3.1 身份与来源

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `id` | entity 标识（如 `<entity id>`）；`--entity` 可直接用它 | ✅ |
| `class` | 本体 class（如 `<class>`）；`--entity` 也接受它 | ✅ 进 `@type` = `ontok:<class>` |
| `source` | 指向 `sources[].id` | ✅ 不存在 ⇒ `am.invalid` |
| `reviewed_by` | **谁签的这份映射**（`<签署人>@<YYYY-MM-DD>`） | ✅ **门禁**：缺失/空串 ⇒ `am.unsigned`（退出 2），未签**一律不许取数** |

> 未签取不了，这不是「配置没填完」的软提示，而是**硬门禁**：`reviewed_by` 为空时脚本连一次请求都不发。

### 3.2 `operation`：调哪个接口

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `action` | 接口名（如 `<接口名>`） | ✅ 进签名器要求的「动作」头；规范签名里**转小写** |
| `version` | 接口版本（如 `<接口版本>`） | ✅ 进签名器要求的「版本」头 |
| `method` | HTTP 方法 | ◐ **只实现 POST**（别的 ⇒ `am.invalid`） |
| `content_type` | 请求体类型 | ◐ 缺 ⇒ 签名器默认（`application/json; charset=utf-8`）；**签名用的就是发出去的那个值** |
| `path` | 请求路径 | ◐ 缺 ⇒ `/` |
| `headers` | header 模板（`{动作}`/`{版本}`/`{时间戳}`/… 都是占位符） | ◐ **声明了就用它**；没声明才补签名器标准头（见 §五） |
| `vars` | 填 header 模板的值表 | ◐ 声明了 `headers` 才有用；写了脚本会拿去填 |

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
| `source = filter` 的名字 | `--param` 给的过滤值 |
| `source = default` 的名字 | `sources[].defaults` → `request.defaults` |
| `{offset}` / `{limit}` | 分页器（不是模板占位符，见 §3.5） |

**两条脚本额外遵守的规则**：

1. **`--param` 覆盖同名默认值**（显式 > 默认）。否则某个参数永远只能取 AM 里写死的默认值，`pushdown` 就没意义了。
2. **逗号只在 `shape = "array"` 时才当分隔符**。声明了 `shape: array` 的参数，`--param <名>=1,2,3` 会变 `[1,2,3]`；**没声明 shape** 的多值参数，`--param <名>=a,b` 会整串当一个值（见 §六「待办」）。

### 3.4 `iterator` / `fields` / `identity` / `iri_template`：怎么切行、怎么映射

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `iterator` | 响应里「一行实体」的路径 | ✅ **JSONPath 子集**：只支持 `$.a.b[*]` 与 `$.a.b`（`$.a.b.c[*]` 也可以）。数组下标 `[0]`、`[*]` 在中间、`..`、过滤器一律 `am.invalid` 并说明改法 |
| `identity.key_path` | 主键在一行里的位置 | ✅ 取出来填 `iri_template` 的 `{id}`；没有 identity 的行会被跳过并记 `meta.skipped_no_identity`（不是静默丢） |
| `iri_template` | `@id` 模板，必须含 `{id}` | ✅ 缺失 `{id}` ⇒ `am.invalid` |
| `fields.<属性>.path` | 属性 ← 一行里的点号路径 | ✅ 逐层下钻；某行缺这条路径 ⇒ 该属性没有值，**不丢整行**（RECIPE §六） |
| `fields.<属性>.type` | 属性类型（`xsd:integer` 等） | ⛔ 脚本原样透传 JSON 值，不做类型转换 |

### 3.5 `paging`（实体级）与 `request.paging`（请求级）

AM 里**两份可以并存**，各管一件事：

| 键 | 在哪 | 是什么 | 脚本 |
| --- | --- | --- | --- |
| `offset_param` | `entities[].paging` | 偏移/页码参数名（§五 有兜底默认值） | ✅ 决定 body 里哪个键吃偏移 |
| `limit_param` | `entities[].paging` | 每页条数参数名 | ✅ 同上 |
| `max_limit` | 两处都有 | 服务端单页上限 | ✅ 两处都读；`--limit` 会被它夹住（`min(--limit, max_limit)`） |
| `total_path` | `entities[].paging` | 响应里总数的路径（`$.<字段>.<总数>`） | ✅ **有它就按它判「取全」**；没有就按「短页/空页」判 |
| `offset` / `page` | `request.paging` | 同上两个名字（v2 写法） | ✅ 与实体级互为兜底 |
| `value_type` | `request.paging` | `number` / `string` | ✅ **实测必须看**：有的接口要求 `Offset`/`Limit` 是**字符串**，发数字会被拒（RECIPE §四） |
| `offset_base` | `request.paging` | `0` / `1`；页码从 1 起时写 `1` | ✅ 缺省：有 `page` ⇒ 1，否则 0 |

翻页终止（脚本实现，与 RECIPE §四一致但更严）：

| 情况 | 处理 |
| --- | --- |
| 有 `total_path` 且已取到总数 | 取满就停（**精确**） |
| 没有总数 | 本页条数 < 本页 `limit` 就停；空页也停 |
| 到 `--max-rows` | 停，**`truncated=true` + 退出 8** |
| 到 1000 页保险丝 | 停，**`truncated=true` + 退出 8** |

⛔ 没有「静默截断」这条路：只要少取了，`meta.truncated` 一定是 `true`，退出码一定是 `8`。

### 3.6 `capability`：能下推什么

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `pushdown` | **服务端能筛**的字段名（**白名单**） | ✅ `--param` 只放行这里面的名字；给了白名单外的 ⇒ `param.not_pushdown`（退出 2）并列出可用参数 |
| `local_only` | 只能拉回来在本地筛的字段 | ⛔ 脚本**不筛**：它只负责取回原始行，筛选是调用方的事（否则「看起来过滤了、其实是全量」更危险） |
| `max_scan_rows` | 单次扫描的行数建议上限 | ⛔ 脚本不拿它当硬限制（硬限制是 `--max-rows`，默认 1000） |
| `cost_hint` | 计费提示（`free` / …） | ⛔ 只给人看 |

### 3.7 `freshness` / `fixture`

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `freshness.mode` / `.note` | 数据新鲜度 / 口径说明 | ⛔ 纯文档，脚本不读（它是**声明**，不保证这次的数据是新的） |
| `fixture` | 一份**真实形状的响应**（离线夹具） | ◐ `--dry-run` 用它取数（**不发网络、不要凭据**）；`--check` 检查文件在不在（缺了只是 warning） |

> 夹具的**来源**必须看清：有的夹具是真实响应脱敏，有的是照语料合成的**自证夹具**。
> `--dry-run` 通过只说明「映射与夹具自洽」，⛔ 不等于「真接口能取」。

---

## 四 · `relations`

| 键 | 是什么 | 脚本 |
| --- | --- | --- |
| `relations[]` | 实体间关系（`from` / `to` / 证据等级…），用于跨源遍历 | ⛔ **完全不读** |

取数是「一个 entity → 一次接口」；跨源边（一个类在库、一个在 API）要拆两段走，
那段编排在调用方（`SKILL.md` §4.3 末尾），不在这个脚本里。

---

## 五 · 脚本自己补的东西（签名器默认值）

这些默认值属于**签名器**（`SIGNERS` 里那个类），不属于主流程 —— 换一种算法就换一个签名器，
默认值跟着它走。`--check` 每次都会打一条提示说明哪些 source 正在用默认值。

| 缺什么 | 谁来兜底 | 为什么可以这样 |
| --- | --- | --- |
| `auth.signed_headers` | 签名器默认（`tc3-hmac-sha256`：`content-type;host;x-tc-action`） | 与签名规格的官方向量同一组头；少了「动作」头反而签不过 |
| `auth.timestamp` | 签名器默认（`X-TC-Timestamp` / `unix-seconds`） | 该算法约定的唯一形态 |
| `auth.scope` | 签名器默认（`date/service/tc3_request`） | 同上 |
| **`auth.service`** | ⛔ **不兜底**：缺 ⇒ `am.invalid`（退出 2）并点名 | 它是签名输入；猜错不报错，只会签出「看起来对」的错串 |
| `operation.headers` | 补签名器要求的头（`tc3-hmac-sha256`：`Content-Type` / `X-TC-Action` / `X-TC-Version` / `X-TC-Timestamp`） | AM 只声明了 action/version，头是算法要求的 |
| `operation.path` | `/` | 该 API 形态恒 `/` |

**⚠️ 「一个值只放一处」（脚本比 AM 保守的地方）**：
`Region` 这类参数已经在 `request.template` 里进 body 时，脚本**不会**再补同名 header ——
同一个值同时进 header 与 body 会得到 `UnknownParameter`（RECIPE §七①，真调多个类全中）。
反过来，如果某个 entity 的 body 里没有它而有对应取值，脚本才补那个 header。
改 AM 时要记住这条：**一个值只放一处**。

---

## 六 · 已知限制与 AM 作者待办

脚本**不偷偷砍需求**，做不到的都在这儿写明：

| # | 限制 | 影响 | 建议 |
| --- | --- | --- | --- |
| 1 | `kind` 只支持 `http-json` | 别的源类型直接拒绝 | 需要新源类型就先提需求 |
| 2 | 已实现的签名器只有 `tc3-hmac-sha256`（`--list-signers` 查） | 别的 `auth.mode` ⇒ `auth.unsupported`（退出 4），⛔ 不 fallback | 加算法**不用改主流程**：写一个 `Signer` 子类 + 在 `SIGNERS` 注册一行（`fixtures/example-signer.patch` 有一个 20 行的样子） |
| 3 | 业务错误信封只认 `<信封>.Error{Code, Message}` | 换一种信封形状的源，错误码会被归到 `api.invalid` 一类的兜底 | 需要时扩 `_raise_vendor_error`（按源声明信封形状） |
| 4 | `iterator`/`total_path`/`fields.path` 只支持 JSONPath 子集 | 复杂路径（`$.a[0].b`、`$.a[*].b`、过滤）会被**拒绝**而不是猜 | AM 里把路径落到子集内；实在不行报给脚本作者扩语法 |
| 5 | `paging` 没写参数名时，脚本用 `Offset` / `Limit` 兜底 | 这两个名字是**某类 API 的命名**；换一家可能不对 | 每份 AM 显式写 `paging.offset_param` / `limit_param`，别依赖兜底 |
| 6 | 多值参数没声明 `shape: array` | `--param <名>=a,b` 会整串当一个值 | 在 `bindings.<名>` 加 `"shape": "array"`（一行改动） |
| 7 | 不做自动重试/退避 | 限流/超时要调用方自己重试 | `SKILL.md` 里已写明；需要就提需求 |
| 8 | 不把结果推到服务端 | 行只在你手上 | 这是设计，别指望 `@graph` 自动回填 |

---

## 七 · 30 秒自查（改完 AM 之后）

```bash
python scripts/access_fetch.py --list-signers                          # 有哪些 auth.mode 能用
python scripts/access_fetch.py --mapping <am.json> --check              # 结构 + 签名门禁
python scripts/access_fetch.py --mapping <am.json> --entity <id> --dry-run   # 用夹具走一遍
python scripts/access_fetch.py --mapping <am.json> --entity <id> --param <名>=<值> --max-rows 5  # 真取一小页
```

`--check` 只看结构（不取数、不要凭据）；`--dry-run` 要夹具、不要凭据；真取数才要环境变量。
