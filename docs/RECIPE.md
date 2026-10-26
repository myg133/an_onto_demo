# RECIPE · 取数配方怎么用（契约 v2）

> **读者**：拿到 `@pending[].get` 的**外部客户端开发者**。
> 你从我们的服务里拿到的是「**去哪儿取、怎么拼请求、响应里哪一段是数据**」，
> 而不是数据本身 —— 平台**不代你调** API（凭据不出你的边界）。
>
> 本文只讲**怎么用这份配方**。签名算法单独一篇：`docs/AUTH-SPEC.md`。
> 文中示例取自一份真实的云 API 映射（地域 `ap-guangzhou`），并**按配方独立实现客户端真调过**；
> `⛔` 标记的都是实测踩过的坑，不是风格建议。

---

## 一 · `get` 有 10 个键，哪些可缺

`get` 是**加法式**演进的：前 4 个键在任何版本都有，后 6 个是契约 v2 才可能出现的取数配方。

| # | 键 | 内容 | 可缺？ |
| --- | --- | --- | --- |
| 1 | `class` | 本体里的类 key（`cvm_instance`） | ❌ 恒在 |
| 2 | `filter` | 平台**已经替你确定**的取数标识（如 `{"ids":["ins-1"]}`）；`{}` = 平台不知道 | ❌ 恒在（可能是 `{}`） |
| 3 | `pushdown` | 这个类**服务端能筛**的字段名（`["region","zone"]`） | ❌ 恒在（可能是 `[]`） |
| 4 | `ref` | 溯源指针：`access#<id>@v<mapping_version>` | ❌ 恒在 |
| 5 | `source` | `{ "base_url": …, "auth_mode": … }` | ✅ 映射没声明就没有 |
| 6 | `operation` | `method` / `path` / `headers{}`（header 模板）/ `vars{}` | ✅ |
| 7 | `request` | `template{}`（请求体模板）/ `required[]` / `bindings{}` / `paging{}` / `defaults{}` | ✅ |
| 8 | `iterator` | 响应里「一行实体」的路径（`$.Response.InstanceSet[*]`） | ✅ |
| 9 | `fields` | `属性 → 响应字段路径`（**已拍平**的字符串映射） | ✅ |
| 10 | `auth` | `credential_env[]` / `signed_headers[]` / `timestamp{}` / `scope[]`（**只有声明，没有凭据值**） | ✅ |

**怎么读「可缺」**：第 5–10 个键**不存在**，表示**映射里没声明**这一项 ——
⛔ 不要按 `auth_mode` 去猜签名头，⛔ 不要按类名去猜接口地址。缺了就是缺了，去问映射作者。

**契约 v1 的 `get` 只有前 4 个键**，后 6 个一个都不出现 —— 见到 4 键就说明这份映射还没升到 v2。

一份真实的 `get`（字段表只留 3 条）：

```json
{
  "class": "alarm_policy",
  "filter": {},
  "pushdown": ["module", "policy_name"],
  "ref": "access#AlarmPolicy@v1",
  "source": { "base_url": "https://monitor.tencentcloudapi.com", "auth_mode": "tc3-hmac-sha256" },
  "operation": {
    "method": "POST", "path": "/",
    "headers": { "Content-Type": "application/json; charset=utf-8",
                 "X-TC-Action": "{action}", "X-TC-Version": "{version}",
                 "X-TC-Timestamp": "{timestamp}" },
    "vars": { "action": "DescribeAlarmPolicies", "version": "2018-07-24" }
  },
  "request": {
    "required": ["module"],
    "template": { "Module": "{module}", "PolicyName": "{policy_name}" },
    "bindings": { "module": { "source": "default" }, "policy_name": { "source": "filter" } },
    "paging": { "page": "PageNumber", "limit": "PageSize", "max_limit": 100, "offset_base": 1 },
    "defaults": { "Region": "ap-guangzhou", "module": "monitor" }
  },
  "iterator": "$.Response.Policies[*]",
  "fields": { "policy_name": "PolicyName", "monitor_type": "MonitorType", "enable": "Enable" }
}
```

⚠️ `request.defaults` 是 **CP4 起**才有的键：`bindings.<名>.source = "default"` 只说
「从默认值取」，值本身在 `sources[].defaults` 里 —— 不给你这一项，`{module}` 就没人知道该填 `"monitor"`。
它**只在源声明了 `defaults` 时才出现**；出现时**键名/大小写/嵌套结构原样**（⛔ 我们不合并、不改写）。

---

## 二 · 占位符的值从哪来

模板里的 `{name}` 只有 **五类来源**。把下面这张表合成一张「值表」，header 模板和 body 模板**都从它取**：

| 占位符 | 值从哪来 | 例子 |
| --- | --- | --- |
| `{action}` / `{version}` | `operation.vars` | `"action": "DescribeInstances"` |
| `{timestamp}` | **你的客户端生成**（放 `auth.timestamp.header`，格式按 `auth.timestamp.format`） | `unix-seconds` ⇒ `1750000000` |
| `{region}` / `{zone}` / `{vpc_id}` … | 调用方给的过滤值（`bindings.<名>.source = "filter"`） | `{"zone": "ap-guangzhou-5"}` |
| 任意 `source = "default"` 的占位符 | **`request.defaults`** | `{"module": "monitor"}` |
| `{offset}` / `{limit}` | 分页器（**不是**模板占位符，见 §四） | — |

三条硬规则：

1. **`source = "default"` 的值只能来自 `request.defaults`** —— 别从 `operation.vars`、
   别从 `get.filter`、别从环境变量里「顺手猜一个」。
2. **`request.defaults` 的键名大小写可能与占位符不同**：真实映射里是
   `{"Region": "ap-guangzhou"}` 配 `{region}`。**原样保存**，查找时**先精确、再大小写不敏感**
   （参考实现就是把两边都按小写比较，但**输出**必须用映射里的原键名/原值）。
3. **`request.required` 里的名字必须有值**，否则**拒绝发请求**（见 §五）。

> 为什么 `headers` 和 `template` 共用一张值表：两者在真实接口里经常互相引用同一个值
> （`X-TC-Region: {region}` 与 body 里的筛选条件都来自「当前地域」）。
> ⛔ 但**同一个占位符不要两处都写** —— 见 §七 第 1 条。

---

## 三 · 剪枝规则（⚠️ 必须遵守，否则 API 报错）

`template` 里的占位符**不是每个都有值**（`{vpc_id}` 这种是可选的）。没值时**不能发空值**，
也不能发一个「半截」的对象 —— 要按位置**剪掉**：

| 位置 | 规则 | 为什么 |
| --- | --- | --- |
| 字符串里的某个占位符没值 | **剪掉该值**（这个成员/元素没有值） | 发 `""` 会被当「筛空字符串」，语义完全不同 |
| **数组元素**（对象）里少一个键 | **整个元素剪掉** | `Filters:[{Name:"vpc-id",Values:[]}]` 会被拒：`InvalidParameterValue`；一个过滤器是一条**整体**，缺一半没有意义 |
| **顶层对象**少一个键 | **只丢那个键** | `{Module, PolicyName}` 缺 `PolicyName` 时**必须保住 `Module`** —— 否则整条请求丢必填参数 |
| 数组被剪空 | **整个数组剪掉** | 空数组同样是「一个都没筛」还是「筛了空集」的歧义源 |

补充约定：

- 模板里的占位符应当**独占整个字符串**（`"{zone}"`）。⛔ 不要写 `"prefix-{x}"`：
  这种「拼在中间」的值没有安全的剪法（剪出来是 `"prefix-"`），**直接拒绝这个值/这条请求**。
- 顶层对象被剪空（`{}`）是合法的：表示「本次不带任何过滤」⇒ 取该范围的**全量**（分页取）。
  ⚠️ 别把它读成「没查到数据」。
- 剪枝只发生在**模板**上。`defaults` / `vars` / `required` 本身**不剪**。

---

## 四 · 分页

`request.paging` 的键：

| 键 | 含义 | 缺省 |
| --- | --- | --- |
| `page` | 页码参数名（`PageNumber`）；**1 基** = 第 1 页发 `1` | 与 `offset` 二选一 |
| `offset` | 偏移参数名（`Offset`）；**0 基** = 第 1 页发 `0` | 与 `page` 二选一 |
| `limit` | 每页条数参数名（`Limit` / `PageSize`） | — |
| `max_limit` | 服务端单页上限（**建议值**，取 `min(max_limit, 你要的页大小)`） | — |
| `value_type` | 位置值的 JSON 类型：`"number"`（缺省）/ `"string"` | `number` |
| `offset_base` | `0` / `1`；`page` 缺省 1、`offset` 缺省 0 | 见左 |

**`value_type` 是实测必须看的**：有些接口把 `Offset` / `Limit` 定义成 **String**，
发数字会被拒 —— `InvalidParameter: input type should be string`。VPC / 子网 / 安全组就是这类
（映射里写 `"value_type": "string"`）。

```
value_type = "number"  ⇒  {"Offset": 0,   "Limit": 100}
value_type = "string"  ⇒  {"Offset": "0", "Limit": "100"}
```

**`page` + `offset_base: 1` 表示页码（1 基）**：第 1 页发 `1`，第 2 页发 `2`，⛔ 不是
「已取条数 + 1」。想要不歧义就按 `paging.page` + 默认 `offset_base` 实现。

**翻页终止**：`get` 里**没有**服务端总数字段（`get` 只有 `request`，实体级 `paging.total_path`
不在配方里）。用「**本页返回条数 < 本页 limit**」或「本页为空」判终止；
⚠️ 某些接口即使还有数据也会返回**空页**（权限/地域导致），连续两页空再停更稳。

---

## 五 · `required` 的语义

`request.required` 是**绑定名**（= 占位符名）的列表，表示**没有值就不许发请求**：

- 值可以来自**过滤输入**（`source = filter`）或**默认值**（`source = default`）；
- 两者都没有 ⇒ **在发请求之前就失败**，并明确告诉调用方缺哪个名字。
  ⛔ 不要「先发一个空值，让服务端报 `MissingParameter`」：那是把可诊断的错误变成一次
  白跑的调用（还可能计费）。
- 真实例子：`clb_listener` 要 `load_balancer_id`、`cost_detail_bill` 要 `month`、
  `security_group_rule` 要 `security_group_id` —— 这些**不是映射写错了**，是你的调用方
  必须从别处（库表 / 用户输入）拿到的业务标识。

---

## 六 · `iterator` 与 `fields`

**`iterator`**：响应里「一行实体」在哪。

- 形状是 `$.A.B[*]`：`$` = 响应根，`.` 逐层下钻，末尾 `[*]` = 取数组的每个元素；
- 一个响应只有**一个**列表是你关心的（不是列表的字段不要写进 `iterator`）。

**`fields`**：属性 → **一行响应对象内**的字段路径。

- 在 `get` 里是**拍平的字符串**：`{ "instance_id": "InstanceId", "zone": "Placement.Zone" }`；
- 在 mapping 文件（`ref` 指向的那份）里是 `{ "path": "InstanceId", "type": "xsd:integer" }` ——
  两份说的是同一件事，`get` 只保留 `path`；
- 路径按 `.` 逐段下钻，**不支持数组下标**（`X.N` 这类属性在列表项里取不到，属于已知限制）；
- 某一行里**缺**某条路径 ⇒ 那个属性按「没有值」处理（跳过/置空），⛔ 不要因此丢掉整行
  （云 API 里合法可为空的字段很常见）。

⚠️ `get` **不含** `identity.key_path`（主键在响应里的位置）。需要稳定实例标识时，
从映射文件（`ref`）或 `describe` 的属性里取，⛔ 不要拿 `fields` 里随便一个属性当主键。

---

## 七 · 常见坑（实测）

### ① 同一个占位符既进 header 又进 body（`UnknownParameter`）

`Region` 这类**只能走 header** 的参数，如果同时在 `request.template` 里写一份：

```
✗ UnknownParameter · The parameter `Region` is not recognized.
```

真调 11 个类全中，而**离线体检原先一路绿灯**（两处各自都合法）。
⇒ 现在 `ontok-builder access validate` 会给一条 **warning**（`DUPLICATE_PLACEMENT`，
不阻断）：同一个占位符同时出现在 `operation.headers` 与 `request.template` 就点名。
看到这条警告，先确认「这个值到底该走哪一边」。

### ② 分页值的类型不对

`Offset` / `Limit` 按接口定义可能是 **String**（见 §四）。传数字的报错是
`InvalidParameter: input type should be string` —— 别去怀疑签名，先看 `paging.value_type`。

### ③ 把「没值」发成「空值」

`Filters:[{"Name":"vpc-id","Values":[]}]` 会被 API 拒（`InvalidParameterValue`）。
必须**整条过滤器剪掉**（§三），而不是保留一个空 `Values`。

### ④ 缺 `required` 还硬发

`MissingParameter · The request is missing the required parameter 'Module'` ——
这条错误的意思是「你的调用方少给了一个业务值」，不是「平台少给了默认值」。先查
`request.required` + `request.defaults`（§五）。

### ⑤ `defaults` 的键名大小写与占位符不一致

`{"Region": "ap-guangzhou"}` 配 `{region}` 是**正常**的：契约原样透出、不做小写化。
查找时先精确、再大小写不敏感；⛔ 不要把 `Region` 改写成 `region` 再回写映射。

### ⑥ 签名的 body 哈希用的是「你以为要发的 body」

签名（`docs/AUTH-SPEC.md` §3）对的是**实际发出的字节**。剪枝发生在签名之前：
先剪枝 → 序列化 → 再用这串字节算哈希/发送。⛔ 先签名、后改 body ⇒ 必报签名不匹配。
