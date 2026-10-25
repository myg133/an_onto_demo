# 需求：REQ-149 · 分页与过滤绑定的**通用性**（3 个缺口）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-149 |
| 优先级 | **P0**（挡住 AM 真调用） |
| 状态 | 已就绪（未开工） |
| 基线 | `develop` `6f4987d` + `feature/REQ-144-stage1`（依赖它） |
| 触发 | 生成 14 个 entity 的 AM 时实测出的执行器缺口 |

## 三个缺口（**都是通用能力，不是厂商特例**）

### A · 分页参数**类型**不可控

**现状**：`net.rs::build_payload` 对 `paging.offset/limit` 固定写 `json!(n)`（数字）。
**后果**：VPC `DescribeVpcs/Subnets/SecurityGroups` 的 `Offset/Limit` 文档类型是 **String**，
模板里写 `"{offset}"` 也会被覆盖成数字 ⇒ 真调用可能被拒（`InvalidParameter`）。

**要**：分页/绑定支持**显式类型**：

```json
"paging": { "offset": "Offset", "limit": "Limit", "value_type": "string" }
```

或等价机制。默认仍 `number`（⛔ 不改变现有行为）。

### B · 分页**起点**不可控（0-based vs 1-based）

**现状**：执行器把 0 起的 offset 原样写入。
**后果**：腾讯云 monitor 的 `PageNumber` **从 1 开始** ⇒ 第一页发 `PageNumber=0`，行为未定义。

**要**：`paging.offset_base: 0 | 1`（默认 0）。

```json
"paging": { "page": "PageNumber", "size": "PageSize", "offset_base": 1 }
```

### C · 过滤值只能是字符串 ⇒ `Array of Integer` 无法下推

**现状**：`bindings[].source = filter` 产出的值一律是字符串（单值）。
**后果**：`SSL CertificateStatus.N`、`CDB InstanceTypes.N/Status.N` 这类 **整数数组**筛选无法正确下推
⇒ 只能标 `local_only`（拉回来自己筛）。

**要**：绑定支持声明**值类型与形状**：

```json
"bindings": {
  "status": { "source": "filter", "value_type": "integer", "shape": "array" }
}
```

## 不做

- ⛔ 不做"按接口 id 的特例分支"（与本需求同一原则：只加**通用表达力**）
- ⛔ 不改 `template` 的其余语义（嵌套/数组/剪枝规则不变）
- ⛔ 不改阶段 0/1 的默认行为

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | 三个新字段**全部可选**，默认值保持现状 ⇒ 旧 mapping 行为逐字节不变 |
| R2 | 变体必须能**红**：去掉 `offset_base=1` ⇒ 断言 `PageNumber=0` 出现 |
| R3 | 类型/形状的**取值集合要封闭**（`number|string` / `string|integer|boolean` / `scalar|array`），⛔ 不收任意字符串 |
| R4 | 不支持的类型组合 ⇒ **报错并点名**，⛔ 不许静默降级成字符串 |

## 交付物

1. 三个能力 + 各自判据（含变异红）
2. 用 `am-tencent.json` 的 VPC / monitor / SSL 三个 entity 做**形状验证**（假端点收到的 body）
3. 旧 mapping 零变化证明
