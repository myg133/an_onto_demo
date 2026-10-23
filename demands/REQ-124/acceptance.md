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


---

## 三 · Dev 交付与裁定（K-1 ~ K-4）

### K-1 · TTL 导出有**两处 render 调用点**：接受，但口径要写死

Dev 自报：CLI `ontology export` 走 `publish_customer`，控制面走 `console::ops::render_ttl_export`。

**根因**：`publish_customer` 命中发布闸会 `process::exit(2)` —— **服务进程不能因为一次导出请求退出**。
而 REQ-107 的 T2/T8 要求「写点与管线调用同函数」。

**裁定：接受**，因为**实现是共用的**（`prepare_ttl_export` 的投影 + `domain::pipeline::render` + 同一个序列化器），只有**末端 publish/exit 的包装**不同。

**但必须满足**：

1. 报告里列明「哪一段共用、哪一段不同」（Dev 已列）
2. **自动对拍**持续存在：`test_req124_live` 必须断言 TTL（客户/建模）与 R2RML（含 `--include-inherit`）的 sha256 与 CLI **相等**
3. 登记为**已知偏离**：若将来有人把 T2/T8 改成「一个调用点」，必须同时解决「服务进程不许 exit」——这是**需求冲突**，不是疏忽

⛔ 不许把它写成「完全一致没有偏离」。

### K-2 · 验收口径用「产物 + 输入」而不是「调用点数量」

- 判据：`sha256(API 产物) == sha256(CLI 产物)`，且两者消费同一份 `TtlPrepared`
- 判据：`console/ops.rs` 里**没有一条写 SQL**（Dev 说只有两个写调用：apply/freeze）

### K-3 · 三处设计取舍：接受

| # | 项 | 裁定 |
| --- | --- | --- |
| 1 | `POST /api/import` 的内联 `proposal.project` 仍必填且须等于请求 project | **接受**（AC-003 明写「缺 project ⇒ 400」） |
| 2 | `GET /api/export/ttl` 的 `schema`/`tenancy` 是**服务端路径** | **接受**（与 CLI 同义；admin 本就能读） |
| 3 | `POST /api/decide` 的 body 比 `review serve` 多 `project`/`proposal`；成功正文 `via` 记 `console` | **接受**：控制面是多提案常驻服务，且 `via` 区分入口是设计 |

⚠️ 但 **GET 侧必须逐字节一致**（Dev 实测 1076 B sha 相同，QA 要复核）。

### K-4 · 审计范围：接受当前口径

Dev：审计覆盖**写操作 + export/r2rml + 鉴权失败**；`GET /api/projects` / `GET /api/review` 不记（避免读放大）。

**裁定：接受**，但必须写进文档（哪些记、哪些不记），并保留 `GET /api/audit` 能查到「401/403 次数」这一事实。
