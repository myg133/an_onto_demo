# 需求：REQ-141 · 接口契约（机器可读，版本化）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-141 |
| 优先级 | **P1**（对接 Axon 必需；但可排在 JSON-LD 之后） |
| 状态 | 进行中 |
| 基线 | `develop` **等 REQ-140 合入后**（契约描述的就是 JSON-LD 形状） |
| 触发 | 与 Axon 对接：他们文档只说"上下文子图（JSON-LD/RDF）"，**没说具体形状** |

## 问题

现在形状只存在于**代码和一次口头说明**里：

```json
{ "@context": …, "@graph": […], "summary": {…}, "meta": {…} }
```

对方拿到会问：`summary` 是什么？`@id` 怎么拼？日期为什么是对象？

**手动给一份文档必然漂**——文档说一套、代码做一套，而且 **Agent 根本不读文档**。

## 要做

### 1 · `GET /api/contract`（+ MCP resource `ontok://contract`）

```json
{
  "contract_version": "1",
  "jsonld": {
    "shape": { "type": "object", "required": ["@context", "@graph", "meta"], "properties": {…} },
    "example": { …一份真实最小响应… },
    "notes": {
      "graph":      "扁平 @graph；同 @id 合并属性（这是子图而不是表的原因）",
      "iri":        "ontok:<类>/<主键>；@context 里有 ontok 前缀",
      "literals":   "字符串/数字/布尔用原生 JSON；日期用 {/@value,@type/}（JSON 无日期类型）",
      "aggregates": "聚合与 GROUP BY 进 summary，不进 @graph（⛔ 不造假节点）",
      "meta":       "warnings/coverage/truncated/join_states 必须原样转达"
    }
  },
  "compatibility": {
    "policy": "只增不减",
    "breaking": false,
    "changelog": [ { "version": "1", "date": "2026-10-24", "change": "首版" } ]
  }
}
```

### 2 · 版本号出现在三处

| 位置 | 用途 |
| --- | --- |
| `GET /api/contract` → `contract_version` | 权威来源 |
| `GET /api/health` → `contract_version` | Agent 启动时比对 |
| 每次响应 `meta.contract_version` | **每次调用都能比对**，能当场发现"对方升级了" |

### 3 · 契约**不能漂**（核心）

手写契约 + 实现 = 迟早不一致。**用测试把两者钉住**：

> 拿**真实响应**去校验契约里的 `shape`，跑 3 类用例（普通 / 聚合 / 空结果）。

⛔ 不引依赖（不用 schemars）；手写一个只支持需要的 JSON Schema 子集的小校验器。

## 不做

- ⛔ 不做 OpenAPI / Swagger
- ⛔ 不做多版本并存（`contract_version` 只升，旧形状不保留；因为还没有外部消费者锁定）
- ⛔ 不改 JSON-LD 形状本身（那是 REQ-140）
- ⛔ 不引依赖

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | `/api/contract` **免 token**（与 health 同口径：只描述形状，零业务数据） |
| R2 | 版本号三处**必须同源**（同一个常量，⛔ 不许各写一处） |
| R3 | 契约的 `shape` 必须被**真实响应**校验（不是"示例符合契约"，而是"**实现符合契约**"） |
| R4 | 兼容策略写进契约本身（`policy` + `breaking` + `changelog`） |
| R5 | 本体模式与普通模式**都给**同一份契约（形状与模式无关） |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 手写契约与实现漂 | R3 的测试 |
| 2 | 校验器本身有 bug ⇒ 假绿 | 变异：故意改一个必填字段名 ⇒ 测试必须红 |
| 3 | 契约成为 API 后，改形状受限 | 这正是目的——R4 把"只增不减"写进产品 |
| 4 | 提前定契约会锁死设计 | `contract_version` 允许升版；`breaking` 字段显式标注 |

## 交付物

1. `/api/contract` + MCP resource + 三处版本号
2. 契约校验器 + 3 类真实响应校验
3. 变异（改必填字段名 ⇒ 测试红）
4. 已知限制
