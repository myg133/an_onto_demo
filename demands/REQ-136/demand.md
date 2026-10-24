# 需求：REQ-136 · 本体服务模式（无数据源也能起服务）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-136 |
| 优先级 | **P0 紧急**（阻塞一个外部平台的接入） |
| 状态 | 进行中 |
| 基线 | `develop` `5163cfb` |
| 触发 | Ops360 文档类项目**没有关系库**，但对方平台需要「本体接口 + TTL」 |

## 问题（我实测的）

```
query serve 无源库        → ✗ 拒启（启动时必须连源库）
query serve + 空 schema   → ✗ 拒启（schema 里一列都没读到）
query serve + 合成 schema → ✓ 起来了，但：
      catalog: classes=20 · relations=0 · excluded_relations=10
      describe: display_name 为空
```

**根因**：`catalog`/`describe` 走的是 **schema + R2RML 对账**那条路：

- 关系要能映射成 join（child/parent 列必须存在）才列出
- 类名/属性名要能从 schema 反查

而**文档类本体**没有列级映射（关系只有语义，没有 join 列），于是被当成「对账不上」全部排掉。

⇒ 给这类项目用现有接口，对方会拿到**"没有关系的本体"**——比不给更糟。

## 要做

给 `query serve` 加**本体服务模式**：

```bash
ontok query serve --bind 0.0.0.0:8800 --allow-public \
  --tokens tokens.json \
  --project ops360 --version 2 \
  --ontology-only          # ← 新增：不连源库、不做 R2RML、不过 schema 对账
```

### 该模式下

| 端点 | 行为 |
| --- | --- |
| `GET /api/health` | 200（多一个 `mode: "ontology-only"` 字段） |
| `GET /api/catalog` | ✅ **直接读快照**：类、属性、**关系**（含 `joinVerified` 等级；不因「无 join 列」而隐藏） |
| `GET /api/describe` | ✅ display_name / 属性 / 覆盖信息齐全 |
| `GET /api/ontology.ttl[?view=]` | ✅ 与 CLI 导出逐字节相同 |
| `GET /metrics` | ✅ 同 REQ-135 |
| `POST /api/sparql` | ⛔ **503** `NO_DATA_SOURCE`，hint 说明「本部署只提供本体，未接业务库」 |
| `/mcp` 工具 | `catalog`/`describe`/`sample` 正常；`sparql_query` ⇒ 同样 503 语义 |

### 启动约束

- ⛔ **不需要** `ONTOK_SOURCE_DATABASE_URL`
- ⛔ **不需要** `--schema`（给了也不对账）
- ✅ 仍需要 `--project` `--version`（快照）与 `--tokens`
- ✅ 版本必须**已冻结**（沿用既有 fail-closed）

## 不做

- ⛔ 不改普通模式（有源库时行为完全不变）
- ⛔ 不做自动造 schema、不猜 join
- ⛔ 不引新依赖

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | `--ontology-only` 与普通模式**互斥**（同时给 `--schema` ⇒ 报错说明） |
| R2 | 该模式下 ⛔ 不许任何请求尝试连源库（连了就说明实现错） |
| R3 | `catalog` **必须列出关系**（这是本需求存在的理由）；`joinVerified=false` 的边也列，但标等级 |
| R4 | `sparql_query` ⇒ **503**（不是 500、不是空结果）——「没有数据源」与「查不到」必须区分 |
| R5 | 普通模式**零行为变化**（回归判据） |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 两个模式的 catalog 逻辑分叉后漂移 | 共用同一个「从快照取类/关系」的函数，只在**过滤**处分支 |
| 2 | 该模式被误用于有库项目 | 启动横幅明确印 `mode=ontology-only（不接业务库）` |
| 3 | 503 被客户端当成"重试就好" | hint 写明「本部署没有数据源，重试无效」 |

## 交付物

1. `--ontology-only` + 上述六类端点行为
2. 用 **ops360 真实快照**跑通（20 类 / 10 关系 / 67 属性 / TTL）
3. 普通模式回归（anspire 快照，catalog/TTL/SPARQL 与之前一致）
4. 已知限制
