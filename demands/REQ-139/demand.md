# 需求：REQ-139 · `/api/health` 报出「我是哪个项目、哪个本体版本」

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-139 |
| 优先级 | **P2**（多套部署前必须；单套时是"少了更麻烦"） |
| 状态 | 进行中 |
| 基线 | `develop` `93e6af2` |
| 触发 | 用户问「UAT 里有哪些 TTL、agent 怎么知道」 |

## 问题

当前 `/api/health` 返回：

```json
{ "service": "ontok-query", "version": "0.1.0",
  "mode": "ontology-only", "syntax": "sparql-v1",
  "protocols": ["rest","mcp"], "transports": ["http","sse"] }
```

**没有 `project`、没有本体版本。**

后果：

| 场景 | 现在 |
| --- | --- |
| Agent 面对一个 `base_url` | 只能**先下 21 KB TTL**、读 `ontok:sourceProject` 才知道是哪套本体 |
| 多套部署 | ⚠️ 无法在不取 TTL 的情况下分辨 |
| 排障 | ⚠️ 看到 503 不知道是哪套本体在拒 |
| **误导** | `version: "0.1.0"` 是**产品版本**，很容易被误读成"本体 v0.1.0" |

## 要做

`GET /api/health`（三个服务都该有，**至少 query**）增加：

| 字段 | 含义 | 例 |
| --- | --- | --- |
| `project` | 项目名 | `"ops360"` |
| `ontology_version` | **本体**版本号（整数） | `2` |

**同时保留** `version`（产品版本），但要在 `--help` / 文档里**说清两者区别**。

### 硬约束

| # | 约束 |
| --- | --- |
| R1 | 免 token（health 本来就免） |
| R2 | ⛔ 不泄露业务数据：项目名可以给（它已经出现在 TTL 与 catalog 里），**不许**加租户 id / 连接串 / 表名 |
| R3 | 普通模式与本体模式**都要有**这两个字段 |
| R4 | 与 `catalog` 返回的 `project` / `version` **必须一致**（同一个数，不能两处各算） |
| R5 | 现有字段**一个都不许少**（有客户端在按 `mode` 判断） |

## 不做

- ⛔ 不做"多套部署的目录服务"（那是另一件事：`deployments.json` 靠运维维护）
- ⛔ 不改 catalog/describe 的响应
- ⛔ 不引依赖

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 破坏现有 health 消费者（如 k8s 探针按字节比对） | R5 + 回归：现有字段原样保留 |
| 2 | `ontology_version` 与 catalog 的 `version` 不一致 | R4 + 测试断言两处相等 |
| 3 | 有人把 `project` 当敏感信息 | 它已出现在 TTL/catalog，公开无害；但仍写进文档说明 |

## 交付物

1. 两个字段 + `--help` 说明产品版本 vs 本体版本
2. 本体模式 / 普通模式各一次实测
3. 与 catalog 一致性断言
4. 已知限制
