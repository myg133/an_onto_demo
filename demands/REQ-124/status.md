# 状态：REQ-124 · 控制面 API

## 当前状态

**进行中**（2026-10-23 开单）

## 为什么

现在 HTTP 能访问的**全是读**；管理流水线（import / apply / freeze / export / r2rml）**只有 CLI**。
⇒ 前端做不了、CI 调不了、别的系统接不了。

## v1 端点

| 方法 | 路径 | 对应 CLI |
| --- | --- | --- |
| GET | `/api/health` | 免 token |
| GET | `/api/projects` | 项目/版本/冻结/计数 |
| POST | `/api/import` | `propose import` |
| POST | `/api/apply` | `proposal apply` |
| POST | `/api/freeze` | `version freeze` |
| GET | `/api/export/ttl` | `ontology export` |
| POST | `/api/r2rml` | `r2rml generate` |
| GET/PUT | `/api/config/{exclusions,policy,tenancy}` | 声明读写（**保存前校验**） |
| GET | `/api/audit` | 审计 |
| GET/POST | `/api/review`、`/api/decide` | 复用领域函数 |

## 不做（明确边界）

- **不跑抽取**：读源库 + 调模型 + 1-2 分钟长任务 ⇒ 属任务系统，另开需求
- 不做前端页面（REQ-125）、不做客户门户（REQ-126）
- 不做多用户/角色体系

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | admin token 与 query token **分开发放**；query token 调管理端点 ⇒ **403** |
| R2 | 默认只绑回环；公网需 `--allow-public` + 警告 |
| R3 | 每个写端点**调既有领域函数**，⛔ 不许 HTTP 层重写 |
| R4 | 声明保存前校验，失败**拒绝保存且原文件不变** |
| R5 | 重复 apply / 冻结后 apply ⇒ **409** |
| R6 | 审计记摘要，⛔ 不记 token 明文 |

## 待办

- [ ] Dev 实现
- [ ] QA（重点：token 分离、声明校验、与 CLI 逐字节相同）
