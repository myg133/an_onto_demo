# REQ-144 · access mapping 的离线夹具

> 本目录的 fixture **全部是合成数据**（⛔ 不含任何真实实例 ID / 账号 / 凭据）。
> 形状照官方文档的响应结构；值是造的，但**字段名、嵌套、类型**按文档写。

## 文件清单

| 文件 | 是什么 | 谁用它 |
| --- | --- | --- |
| `access-cvm.json` | ⭐ 作者格式的 access mapping（1 源 / 2 entity / 1 关系） | `ontok access validate` / `fetch` / `publish` |
| `access-ontology.ttl` | 这份示例要用的**最小本体**（两个类的 `ontok:instanceIri`） | 上面三条命令的 `--ontology` |
| `tencent-cvm-describe.json` | 腾讯云 `DescribeInstances` 形状的响应，**250 行** | `entities[cvm_instance].fixture` |
| `tencent-monitor-alarm-histories.json` | 腾讯云 `DescribeAlarmHistories` 形状的响应，**3 行** | `entities[alarm].fixture` |
| `access-queries.rq` | 发布态 `access.jsonld` 的 SPARQL 查询示例 | 证明映射可查（**额外自证**，⛔ 不是 AC-009） |
| `gen_access_fixtures.py` | 生成上面两份响应的脚本（**幂等**：连跑两次 sha 相同；库里的两份文件就是它的产物） | 想改行数 / 分布时 |

## 两份响应夹具的口径（对应设计稿 §六 的三条纪律）

| 项 | `tencent-cvm-describe.json` | `tencent-monitor-alarm-histories.json` |
| --- | --- | --- |
| 来源 | 腾讯云官方文档 `DescribeInstances` 的 `Response` 结构（`TotalCount` / `InstanceSet[]` / `RequestId`） | 同上，`DescribeAlarmHistories`（`TotalCount` / `AlarmHistorySet[]` / `RequestId`） |
| 什么时候取的 | **没有取过** —— 这是合成夹具，不是脱敏真实响应 | 同左 |
| 哪个 region | `ap-guangzhou`（`Placement.Zone` 在 `ap-guangzhou-2` / `-3` 之间交替：**奇数行 `-2`、偶数行 `-3`**，各 125 行） | 无 region 字段 |
| 脱敏做了什么 | 不需要：ID 是 `ins-0001..ins-0250`、名称是 `web-0001..`，账号 / UIN / 密钥一个都没有 | 同左（`alarm-0001..0003`） |
| 行数与分布 | 250 行；`InstanceState=STOPPED` 83 行（`i % 3 == 0` ⇒ 3,6,…,249）、`RUNNING` 167 行 | 3 行；`AlarmStatus` = `ALARM` 2 行 / `OK` 1 行；`AlarmObject` = 实例 ID |

### 夹具顶层那个 `_comment` 是什么

两份响应夹具的顶层都有一个 `_comment` 字符串数组：

- 它**不是**云 API 的返回字段，是 `gen_access_fixtures.py` 写进去的**溯源说明**
  （来源、行数与分布、`⛔ 不含真实数据`）—— 真实调用**不会**出现它；
- ⇒ 任何 mapping 都⛔ 不许把 `_comment` 当成响应路径（`validate` 的第四条检查也不会因为它在就放过什么）；
- 它同时是「这件夹具是**脚本产物**、不是手改的」的凭据：
  `python fixtures/gen_access_fixtures.py` 跑两遍 sha 必须相同；与库里的版本不同 ⇒ 有人手改过。

> ⚠️ **诚实登记**：设计稿 §六 说「fixture 是**脱敏后的真实响应**，不是编的——编的会漏字段」。
> 阶段 0 拿不到客户凭据（也不该拿），所以这两份是**按官方文档结构手造的合成夹具**。
> 代价写在这里：**它可能漏字段**（真实响应里还有 `SystemDisk` / `InternetAccessible` 等嵌套），
> 而 `validate` 的第四条检查只证明「映射里的路径在**这份**夹具里存在」，
> ⛔ 不证明「在真实响应里存在」。阶段 1 拿到客户脱敏真响应时，应当**整份替换**这两份文件
> （`gen_access_fixtures.py` 只用来造数据，不是契约的一部分）。
>
> **阶段 1 登记（2026-10-25）**：本轮**仍然没有**任何真实 API 调用（没有只读 AK/SK），
> ⇒ 这两份夹具**未被替换**，`gen_access_fixtures.py` 生成的 sha 与阶段 0 相同。
> 阶段 1 的判据因此**完全不依赖**它们：签名对拍用腾讯云文档的官方向量
> （`src/access/tc3.rs` 的单测），分页/限流/超时用**本地假端点**
> （`tests/test_req144_network.rs`，⛔ 不联网）。拿到真响应的那一轮必须回来替换，
> 并在本节记「已替换 + 时间 + 来源」——⛔ 不许把「合成夹具自洽」说成「真接口验证过」。

## `access-ontology.ttl` 为什么单独存在

API 源的类**还不在客户本体里**（REQ-145 才把 `sourceKind: api` 的类接进 catalog/describe）。
阶段 0 要能离线验 `iri_template` 与 `ontok:instanceIri` 是否同形，就得有一份**声明了那两个类**的本体。

这份文件的 `ontok:instanceIri` 与本仓序列化器（`domain::ttl`）的输出**逐字同形**：

```
ontok:instanceIri "https://ontok.local/{project}#{class}/{id}" .
```

真实导出（`ontok ontology export --project <p> --version <v>`）里它长这样（逐字节摘录）：

```
ontok:instanceIri "https://ontok.local/ops360#{class}/{id}" .
ontok:instanceIri "https://ontok.local/anspire_r2rml_excl#{class}/{id}" .
```

⇒ 把 `--ontology` 换成真实导出的 TTL，只要类名对得上，检查逻辑一字不改。

## `access-cvm.json` 的 `request` 为什么是「模板」（2026-10-25 阶段 1 复审后）

真调用（QA 用只读账号）打了一次带过滤的 `DescribeInstances`，服务端回：

```
API_ERROR · 服务端返回码 `UnknownParameter`：The parameter `Zone` is not recognized.（HTTP 200）
```

⇒ **签名完全正确，参数位置错了**。原来那份 `"from_filter": { "zone": "Zone" }` 只能表达
「过滤名 → 顶层标量参数」，而该接口要的是 `Filters[].Name=zone` 这种**数组里放对象**的形状。

结论不是「给这个接口加个特例」，而是**格式表达力不够**：现在 `request` 段是
**任意 JSON 模板 + 占位符绑定**（`template` / `bindings` / `paging`），见
`ontok access fetch --help` 与 `src/access/mod.rs` 的 `RequestSpec` 文档。

⚠️ 两件事分开记：

| 问题 | 归属 |
| --- | --- |
| 映射里把 `Zone` 写成顶层参数 | **抽取端**：语料只有「数据结构」页、没有「接口入参表」，写映射的人只能猜 |
| 格式连「数组对象」都表达不了 | **产品**：已修（模板化），⛔ 代码里没有任何厂商/接口名分支 |

⟹ **签名对 ≠ 接口通**。这一条是那次真调用买来的，⛔ 别把「签名向量逐字节相同」
读成「真接口验证过」。
