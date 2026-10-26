# AUTH-SPEC · API 源的**可执行**鉴权规范（契约 v2）

> 给**客户端实现者**：你拿到一份 `@pending[].get` 配方（或一份 access mapping 的 `entities[]` + `sources[]`），
> 想自己把数据取回来。这份文档说清「怎么把请求签出来」。
>
> - 覆盖算法：**TC3-HMAC-SHA256**（`auth.mode = "tc3-hmac-sha256"`）。这是 v2 唯一有参考实现的算法。
> - 本仓库对应代码：`src/access/tc3.rs`（**构建面**，⛔ 不在客户镜像的服务 bin 里 —— 服务面只描述、不签名）。
> - 契约层：`src/access/mod.rs` 的 `Auth` / `Operation`。⛔ 产品代码里没有任何厂商 header 名，
>   header 名**全部**来自映射 —— 所以这份文档也是唯一需要看厂商细节的地方。

---

## 0 · 一分钟版

```
1. 从映射拿：base_url、operation.headers（模板）、operation.vars、auth.signed_headers、
   auth.timestamp.header/format、auth.scope、sources[].auth.credential_env
2. 拼 header：模板里的 {name} 用 vars[name] 填；再补 auth.timestamp.header = 时间戳
3. 规范请求串（§3）→ 它的 SHA256
4. 待签名字符串（§4）= 算法名 \n 时间戳 \n scope \n 上一步的哈希
5. 派生密钥（§5）→ 签名 = HMAC(密钥, 待签名串)
6. Authorization（§6）
```

⛔ **两个「逐字节」**（签错 90% 出在这两处）：
① 参与签名的 header 值必须与你**实际发出**的一模一样；
② `HashedRequestPayload` 必须是你**实际发送**的 body 字节。

---

## 1 · 从契约里读哪些值

`@pending[].get` 的形态（契约 v2）：

```json
{
  "source":    { "base_url": "https://cvm.tencentcloudapi.com", "auth_mode": "tc3-hmac-sha256" },
  "operation": { "method": "POST", "path": "/",
                 "headers": { "Content-Type": "application/json; charset=utf-8",
                              "X-TC-Action": "{action}", "X-TC-Version": "{version}",
                              "X-TC-Region": "{region}", "X-TC-Timestamp": "{timestamp}" },
                 "vars":    { "action": "DescribeInstances", "version": "2017-03-12" } },
  "request":   { "template": { "InstanceIds": ["{ids}"] }, "required": ["ids"],
                 "bindings": { "region": {"source":"default"}, "ids": {"source":"filter"} },
                 "defaults": { "Region": "ap-guangzhou" },
                 "paging": { "offset": "Offset", "limit": "Limit", "max_limit": 100 } },
  "iterator":  "$.Response.InstanceSet[*]",
  "fields":    { "name": "InstanceName" },
  "auth":      { "credential_env": ["TENCENT_SECRET_ID", "TENCENT_SECRET_KEY"],
                 "signed_headers": ["content-type", "host", "x-tc-action"],
                 "timestamp": { "header": "X-TC-Timestamp", "format": "unix-seconds" },
                 "scope": ["date", "service", "tc3_request"] }
}
```

| 契约字段 | 用在哪 | 注意 |
| --- | --- | --- |
| `source.base_url` | 决定 `Host` | `host` = base_url 的主机名（**含非默认端口**） |
| `operation.method` / `.path` | 规范请求串第 1、2 行 | API 3.0 的 `path` 恒 `/` |
| `operation.headers` | **要发的 header** | 值是模板；`{name}` 从**值表**取（`operation.vars` + `request.defaults` + 调用方过滤值 + 时间戳，见 `docs/RECIPE.md` §二） |
| `operation.vars` | 填模板 | 只做字符串替换，⛔ 不改 header 名 |
| `auth.signed_headers` | 哪些 header 参与签名 | **小写**；列表里的名字必须都出现在你实际发出的 header 里 |
| `auth.timestamp.header` / `.format` | 时间戳放哪个头、什么格式 | 本算法要 `unix-seconds`（UTC 秒） |
| `auth.scope` | scope 由哪几段拼 | 段名顺序即拼接顺序 |
| `auth.credential_env` | **环境变量名** | 只有名字，⛔ 值不在契约里；从部署环境读 |

⚠️ `request` 段是**请求体模板**：`template` 原样 POST（`{name}` 用 `bindings` 指出的来源填：
`default` ⇒ `request.defaults`（契约 v1 里在 `sources[].defaults`），`filter` ⇒ 调用方给的过滤值，
`paging` ⇒ 分页器），`paging.offset` / `.limit` 是**写进模板的路径**，不是值。

⚠️ `X-TC-Region` 这类参数**只走 header**：⛔ 不要同时写进 `request.template`
（实测会报 `UnknownParameter`，离线体检也查不出来；`access validate` 现在会给
`DUPLICATE_PLACEMENT` 警告，见 `docs/RECIPE.md` §七①）。

---

## 2 · 时间戳

- 契约声明 `format = "unix-seconds"` ⇒ 取 **UTC 秒**（`date +%s`）。
- 同一个值放两处：`auth.timestamp.header`（示例 `X-TC-Timestamp`）与待签名字符串（§4）。
- 日期（`date`，进 scope）是**时间戳的 UTC 日期** `YYYY-MM-DD`，⛔ 不是本地日期。
  > 真实坑：东八区 `2019-02-26 00:04:25` 的 PostgreSQL/服务器时间，其 UTC 日期是 `2019-02-25`；
  > 用本地日期拼 scope ⇒ 签名必错，且报错只说「签名不匹配」，看不出是时区。

---

## 3 · 规范化（第 1 步）

### 3.1 规范 header 串

对 `auth.signed_headers` 里的**每一个**名字：

1. 取**实际发送**的值（模板填完 + 时间戳补上之后的值）；
2. 名字转**小写**；
3. 值两端去空白，值内部**连续空格压成一个空格**（HTTP 语义）；
4. 按**名字的 ASCII 升序**排序（`content-type` < `host` < `x-tc-action`）；
5. 每行 `名字:值`，行尾 `\n`（**最后一行也要有**）。

```
content-type:application/json; charset=utf-8
host:cvm.tencentcloudapi.com
x-tc-action:describeinstances
                                          ← 这一行是最后一个 \n 之后的空行
```

⚠️ `x-tc-action` 的值要**小写**（接口名按小写进规范头；实际发送的 header 大小写按模板）。

### 3.2 规范请求串

```
<method>\n
<canonical_uri>\n
<canonical_query>\n
<3.1 的规范 header 串（自带结尾 \n）>\n
<signed_headers 用 ; 连接>\n
<HashedRequestPayload>
```

- `canonical_uri`：API 3.0 恒 `/`。
- `canonical_query`：POST 恒空串。
- `HashedRequestPayload` = `SHA256(body 字节)` 的**小写十六进制**（body = 你实际发送的那串字节，
  包括 `region` / 过滤条件 / 分页参数填完之后的样子）。

⚠️ 规范 header 串**自带结尾 `\n`**，拼接时后面还要再加一个 `\n` ⇒ 规范串里
`query` 与 `content-type` 之间会看到一个空行。**少了这个换行就签不过**。

⚠️ 反过来，**整串的结尾没有额外换行**：上面骨架里每一行末尾的 `\n` 都属于**它自己那一行**
（`<HashedRequestPayload>` 是最后一行，它后面**没有** `\n`）。整串就结束在那个十六进制串的
最后一个字符上 —— 末尾多写一个 `\n`，`SHA256` 立刻变，签名必错。
（`tests/test_req152_auth_spec_vector.rs` 的 `CANONICAL_REQUEST` 常量就是这段字节的逐字节形态：
以 `…8934f064` 结尾，`\n` 一个不多、一个不少。）

---

## 4 · 待签名字符串（第 2 步）

```
TC3-HMAC-SHA256\n
<timestamp 十进制>\n
<date>/<service>/<scope 最后一段>\n
<SHA256(规范请求串) 的小写十六进制>
```

- `TC3-HMAC-SHA256`：算法名（= `auth.mode` 的大写形态；⛔ 不是 `auth_mode` 原样）。
- `service`：产品名（如 `cvm` / `vpc` / `cdb`），必须与 `host` 的产品域名一致。
- `date`：§2 的 UTC 日期。
- scope 的拼法：把 `auth.scope` 的每一段换成实际值后按顺序用 `/` 连接。
  `["date","service","tc3_request"]` ⇒ `2019-02-25/cvm/tc3_request`。
  段名 → 值：`date` = UTC 日期；`service` = 产品名；其余段（`tc3_request`）= **字面量**。

---

## 5 · 派生签名密钥（第 3 步）

```
SecretDate    = HMAC_SHA256("TC3" + SecretKey, date)
SecretService = HMAC_SHA256(SecretDate, service)
SecretSigning = HMAC_SHA256(SecretService, "tc3_request")   ← 最后一段 = auth.scope 的末段
Signature     = HMAC_SHA256(SecretSigning, StringToSign) 的小写十六进制
```

- `SecretKey` 来自 `auth.credential_env` 里**第二个**变量名（第一个是 SecretId）。
  ⚠️ 这条「第几个变量是哪个」的对应关系由**算法**规定，不在契约里 —— 契约只给变量名。
- HMAC 的 key 可以超 64 字节（先 SHA256 再当 key），遵循 RFC 2104。

---

## 6 · Authorization（第 4 步）

```
TC3-HMAC-SHA256 Credential=<SecretId>/<date>/<service>/<scope 末段>, \
SignedHeaders=<signed_headers 用 ; 连接>, Signature=<Signature>
```

放进 `Authorization` 请求头，与 §3 的 header 一起发出。`SignedHeaders` 的顺序与 §3.1 的排序**一致**。

---

## 7 · 自测向量（⛔ 必须先过这一条，再接真接口）

官方文档《签名方法 v3》§1 的请求 + 本契约声明的三头签名集。
`SecretKey` 是文档里那 **32 个字面星号**（不是省略号 —— 它不是打码，能跑）。

### 7.1 输入

| 项 | 值 |
| --- | --- |
| `SecretId` | `AKID********************************` |
| `SecretKey` | `********************************`（32 个 `*`） |
| `service` | `cvm` |
| `host` | `cvm.tencentcloudapi.com` |
| `action` | `DescribeInstances`（规范头里小写 `describeinstances`） |
| `timestamp` | `1551113065`（UTC 日期 `2019-02-25`） |
| `method` / `uri` / `query` | `POST` / `/` / 空 |
| `content-type` | `application/json; charset=utf-8` |
| `signed_headers` | `content-type;host;x-tc-action` |
| body | `{"Limit": 1, "Filters": [{"Values": ["\u672a\u547d\u540d"], "Name": "instance-name"}]}` |

> ⚠️ **body 里的 `\u672a\u547d\u540d` 是「字面的 ASCII 转义序列」**（3 组 `\uXXXX`，共 18 个
> ASCII 字符），⛔ **不是**「先让它解码成中文再参与计算」。要签的是**实际发出去的 body 字节**：
> 这 18 个字符原样在字节里，服务端收到 JSON 后自己会解码成中文。
> 若按「真中文读法」（`未命名`）去算，`HashedRequestPayload` 会变成 `1e07682a…`，
> 与 §7.2 钉住的 `35e9c5b0…` 不符 —— 这类差异不会报「转义写错了」，只会报「签名不匹配」。

### 7.2 中间值与期望结果（逐字节）

```
HashedRequestPayload    = 35e9c5b0e3ae67532d3c9f17ead6c90222632e5b1ff7f6e89887f1398934f064
SecretDate              = da98fb70dcf6b112dc21038d1eeeb3a95c74b4dcb12c1131f864f6066bd02be0
SecretService           = 8d70cbefb03939f929db64d32dc2ba89b1095620119fe3e050e2b18c5bd2752f
SecretSigning           = b596b923aad85185e2d1f6659d2a062e0a86731226e021e61bfe06f7ed05f5af
HashedCanonicalRequest  = 7019a55be8395899b900fb5564e4200d984910f34794a27cb3fb7d10ff6a1e84
Signature               = 10b1a37a7301a02ca19a647ad722d5e43b4b3cff309d421d85b46093f6ab6c4f
```

规范请求串（注意 `x-tc-action` 行后面那个空行）：

```
POST
/

content-type:application/json; charset=utf-8
host:cvm.tencentcloudapi.com
x-tc-action:describeinstances

content-type;host;x-tc-action
35e9c5b0e3ae67532d3c9f17ead6c90222632e5b1ff7f6e89887f1398934f064
```

待签名字符串：

```
TC3-HMAC-SHA256
1551113065
2019-02-25/cvm/tc3_request
7019a55be8395899b900fb5564e4200d984910f34794a27cb3fb7d10ff6a1e84
```

Authorization：

```
TC3-HMAC-SHA256 Credential=AKID********************************/2019-02-25/cvm/tc3_request, SignedHeaders=content-type;host;x-tc-action, Signature=10b1a37a7301a02ca19a647ad722d5e43b4b3cff309d421d85b46093f6ab6c4f
```

> 机判：`tests/test_req152_auth_spec_vector.rs` 用本仓库实现复算上面每个值，并断言
> **本文档里写的期望签名与代码算出来的逐字节相同**（文档与实现在同一轮测试里对拍，
> 改了一边而没改另一边会直接红）。
> 只要你的实现能复现 `Signature = 10b1a37a…6c4f`，就说明 §3–§6 每一步都对了。

---

## 8 · 常见坑（按出现频率排）

| # | 坑 | 表现 |
| --- | --- | --- |
| 1 | scope 用**本地日期**（东八区凌晨） | 签名不匹配，且只在特定时段失败 |
| 2 | `content-type` 签的是 `application/json`、发的是 `application/json; charset=utf-8`（或反过来） | 签名不匹配 |
| 3 | body 先签后改（比如签之前没有 `Region`、签之后补上） | 签名不匹配 |
| 4 | 规范 header 串最后少一个 `\n`（或最后一行没有换行） | 签名不匹配 |
| 5 | header 名没转小写（`X-TC-Action` 进规范串） | 签名不匹配 |
| 6 | `signed_headers` 里列了实际没发的 header（或反之） | 服务端 4xx `AuthFailure.SignatureFailure` |
| 7 | 把 `SecretKey` 打进日志 / 异常信息 | 安全事故；本仓库的实现给 `SignInput` **不派生 `Debug`** 就是这个原因 |
| 8 | 用 `auth_mode` 当算法名（`tc3-hmac-sha256` ≠ `TC3-HMAC-SHA256`） | 服务端拒绝 |

---

## 9 · 契约没给的东西，别自己猜

| 契约里没有 | 正确做法 |
| --- | --- |
| `auth.signed_headers` | **不要**按 `mode` 猜一组默认头 —— 猜出来的头名一旦与实际发送不一致，签名必错。请向平台要一份带 `signed_headers` 的映射（契约 v2）。 |
| `auth.timestamp` | 同上：不知道时间戳放哪、什么格式，就不要硬编一个（v1 契约不声明它）。 |
| `auth.scope` | 同上：段名与顺序影响签名，⛔ 不按算法名反推。 |
| 任何凭据值 | 契约永远只给**环境变量名**。 |

> 这一节是「**只透传、不推断**」的对外说法：`@pending.get.auth` 里没有的键，
> 表示映射作者**没有声明**，不表示「用默认值」。

---

## 10 · 参考实现

- 本仓库：`src/access/tc3.rs`（`sign()` 全链 / `derive_signing_key()` / `finish()`），
  对拍用例：`tests/test_req144_tc3_vectors.rs`（官方向量）、
  `tests/test_req152_auth_spec_vector.rs`（本文档向量）。
- 官方文档（签名方法 v3）：<https://cloud.tencent.com/document/api/213/30654>
- ⚠️ 参考实现属于**构建面**：`ontok`（服务面）编译期不链接它，镜像里也没有它
  （判据：`strings ontok | grep -c 'X-TC-Action'` = 0）。
