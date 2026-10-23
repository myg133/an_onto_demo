# 需求：REQ-124 · 控制面 API（管理流水线的 HTTP 化）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-124 |
| 优先级 | **P0**（前端与自动化的共同前置） |
| 状态 | 进行中 |
| 基线 | `develop` `fbae80a` |
| 前置 | REQ-110/111/113/121/122/123 |

## 问题

现在能通过 HTTP 访问的**全是读**（`serve` 视图、`query` 查询、`review` 审核）。
**整条管理流水线的动作只有 CLI**：

```
propose import / proposal apply / version freeze / ontology export / r2rml generate
```

⇒ 前端做不了、CI 调不了、别的系统接不了。

## 目标：把「每一步 CLI」变成「一个端点」

```
ontok console serve --bind 127.0.0.1:8899 --tokens admin-tokens.json \
    --project anspire_r2rml_excl
```

### 端点（v1）

| 方法 | 路径 | 对应 CLI |
| --- | --- | --- |
| GET | `/api/health` | —（免 token） |
| GET | `/api/projects` | 项目/版本/冻结状态/计数 |
| POST | `/api/import` | `propose import` |
| POST | `/api/apply` | `proposal apply` |
| POST | `/api/freeze` | `version freeze` |
| GET | `/api/export/ttl` | `ontology export`（客户/建模视图） |
| POST | `/api/r2rml` | `r2rml generate` |
| GET/PUT | `/api/config/{exclusions\|policy\|tenancy}` | 读/写三份声明（**保存前校验**） |
| GET | `/api/audit` | 审计流水 |
| GET | `/api/review` · POST `/api/decide` | `review show` / `review decide`（复用领域函数） |

## 范围

### 做

1. `console serve` + 上述端点
2. **管理 token 表**（与查询 token **分开发放**）
3. **声明的保存前校验**（见 R4）
4. **审计**：每个写操作 + 跨租户调试都记一行
5. 默认只绑回环；`--allow-public` 才允许外部地址

### 不做

- ⛔ **不跑抽取**（读源库 + 调模型 + 1–2 分钟长任务，属任务系统，另开需求）
- ⛔ 不做前端页面（REQ-125）
- ⛔ 不做客户门户（REQ-126）
- ⛔ 不做多用户/角色体系（本轮只有 admin）
- ⛔ 不引新依赖

## 关键裁定

| # | 问题 | 裁定 |
| --- | --- | --- |
| R1 | token 模型 | `admin` 与 `query` **分开**；query token 调管理端点 ⇒ **403**。⛔ 不许一个 token 两用 |
| R2 | 绑定 | 默认只绑回环；公网地址需 `--allow-public` + 启动警告 |
| R3 | 写操作一致性 | 每个端点必须**调用既有领域函数**（`propose import` / `apply` / `freeze` 的实现），⛔ 不许 HTTP 层重写一遍 |
| R4 | 声明保存前校验 | 保存即校验，失败**拒绝保存**：表/列必须存在；`main_only` 必须带 `reason`；`exclusions` 的 `category` 合法且 `reason`/`decided_by` 非空；`publicDimensions` 必须带 `reason` |
| R5 | 幂等与冲突 | 重复 `apply` 已 applied 的提案 ⇒ **409**；冻结后再 `apply` ⇒ **409**（沿用 REQ-111 的双闸） |
| R6 | 审计 | 记 `时间 / token 摘要 / 动作 / 目标 / 结果 / 耗时`；⛔ 不记 token 明文 |
| R7 | 错误可行动 | 错误体含 `code` + `message` + `hint`（如「提案已 applied，可改 xxx」） |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 管理端点成了「绕过审核直接改本体」的后门 | 只暴露**既有流程**（import→review→apply），⛔ 不提供「直接改对象」的端点 |
| 2 | 声明被改坏（删掉租户列） | R4 保存前校验；校验失败不落盘 |
| 3 | 管理 token 泄漏 | 与 query token 分离；审计记摘要；默认只绑回环 |
| 4 | 长操作卡住 HTTP | v1 端点都是秒级；⛔ 抽取不进本需求 |

## 交付物

1. `console serve` + 10 组端点
2. admin token 表格式 + 示例（假值）
3. 声明校验规则与拒绝样例
4. 与 CLI **逐字节相同**的导出（TTL / R2RML）
5. 审计样例（无明文）
6. 已知限制
