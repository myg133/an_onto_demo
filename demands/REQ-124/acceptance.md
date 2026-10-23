# 验收标准：REQ-124 · 控制面 API

> **一句话**：把 CLI 的每一步变成端点，但**不许变成后门**——只暴露既有流程，不提供绕过审核的写入口。

## 一 · AC

### AC-001 · 鉴权分离

| 请求 | 期望 |
| --- | --- |
| 无 token | 401（不透露信息） |
| **query token** 调管理端点 | **403**（不是 401、不是 200） |
| admin token | 200 |
| `GET /api/health` | 免 token，只返回版本 |

- 判据：四种各一次；⛔ 不许 admin token 能当 query token 用（反向也要 403/401）

### AC-002 · 项目与版本

- `GET /api/projects` 返回：项目名、版本列表、每版是否冻结、对象/关系计数
- 判据：与直接查库的结果逐项相等（自己写 SQL 对拍）

### AC-003 · 导入

- `POST /api/import` 带有效提案 ⇒ 返回别名（p1）+ 条目数 + 抽取来源
- 路径不存在 / JSON 坏 / 提案缺 `project\|items` ⇒ **400** + 点名
- 判据：三种失败各一次

### AC-004 · 应用与冻结

| 情形 | 期望 |
| --- | --- |
| 正常 apply | 新版本号返回；状态变 applied |
| 重复 apply 同一提案 | **409** + 可读原因 |
| 已冻结 base 上 apply | **409** + 点名版本 |
| freeze 已冻结版本 | **409** |
| freeze 成功 | 状态变 frozen |

- 判据：五种各一次

### AC-005 · 导出与 CLI **逐字节相同**

- `GET /api/export/ttl?project&version&model_view=1&schema=…&tenancy=…` 的响应体 == `ontok ontology export --model-view …` 写出的文件（sha256 相等）
- 客户视图同样
- 判据：两种各一次，贴 sha256

### AC-006 · R2RML 与 CLI **逐字节相同**

- `POST /api/r2rml` 产出 == `ontok r2rml generate …` 的文件（sha256 相等）
- 判据：默认与 `--include-inherit` 各一次

### AC-007 · 声明读写 + 保存前校验

| 情形 | 期望 |
| --- | --- |
| `GET /api/config/policy` | 原文返回 |
| `PUT` 合法 | 200，落盘，回执含 sha256 |
| `PUT` 引用不存在的表 | **400** + 点名表 |
| `PUT` `main_only` 但无 `reason` | **400** + 说明 |
| `PUT` `publicDimensions` 缺 `reason` | **400** |
| `PUT` 非法 JSON | **400** |

- 判据：六种各一次；⛔ 校验失败的 PUT **不得改动原文件**（用前后 sha256 证明）

### AC-008 · 评审端点复用领域函数

- `GET /api/review` 与 `review serve` 的 `/api/review` **同形**（同一次序列化）
- `POST /api/decide` 与 `review serve` 的 `/api/decide` 行为一致（同一领域函数）
- 判据：同一提案在两个服务上读到的条目/建议/双轨状态**逐字段相同**

### AC-009 · 审计

- 每个写操作（import/apply/freeze/config PUT/decide）产生一行：`时间 / token 摘要 / 动作 / 目标 / 结果 / 耗时`
- 判据：⛔ 无 token 明文；失败操作也有一行

### AC-010 · 绑定

- 默认只绑回环；`0.0.0.0` 无 `--allow-public` ⇒ **拒绝启动**
- 带 flag ⇒ 启动 + 警告；横幅打印绑定地址/项目/版本

### AC-011 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` + `serve` 路由 + `query` 路由 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、新增依赖 |
| 基线 | REQ-123 合并后的 lib 563 / doc 4 |
| 出口 | 新端点登记进 `OUTLETS`（内部轨） |

## 二 · 纪律

- **H** 每个写端点必须调**既有领域函数**（`path:line` 指明调的是哪个），⛔ 不许重写
- **G** 变异三元组：让 import 绕过领域函数 / 让 config PUT 跳过校验 / 让 query token 能调管理端点 ⇒ 每条必须被抓
- **S** AC 也会错，发现直接说
