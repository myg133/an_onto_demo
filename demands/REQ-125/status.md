# 状态：REQ-125 · 内部工作台前端

## 当前状态

**进行中**（2026-10-23 开单；等 REQ-124 合并后开工）

## 七个视图

| 视图 | 消费的 API |
| --- | --- |
| 项目与版本 | `GET /api/projects` |
| 评审 | `GET /api/review`、`POST /api/decide` |
| 导出 | `GET /api/export/ttl`、`POST /api/r2rml` |
| 声明编辑 | `GET/PUT /api/config/{...}` |
| 审计 | `GET /api/audit` |
| 查询台 | `POST /api/sparql` |
| 目录浏览 | `GET /api/catalog`、`GET /api/describe` |

## 认证取舍

浏览器导航不能带 `Authorization` 头 ⇒ **页面壳公开、零数据**，口令由用户输入后存 `sessionStorage`，所有 fetch 带 token。

| 规则 | 理由 |
| --- | --- |
| ⛔ localStorage | 关浏览器不失效，共享机器上等于长期留钥匙 |
| ⛔ URL 参数 | 进历史/日志/Referer |
| 壳内零数据 | 与 REQ-110 同一纪律：壳不含任何项目/对象信息 |

⚠️ 这套只用于**内网 + 回环 + 管理员**；客户门户（REQ-126）不能用同一套。

## 硬判据

- 查询台**必须显示** `warnings` / `coverage` / `join_states` 的 `used=false` / `truncated`
- 危险操作（apply/freeze/声明保存）二次确认，写明目标与后果
- 声明保存失败**保留用户输入**
- XSS：用户数据一律 `textContent`；CSP 头
