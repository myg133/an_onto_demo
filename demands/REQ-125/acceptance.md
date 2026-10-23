# 验收标准：REQ-125 · 内部工作台前端

> **一句话**：能点、能看、不泄漏；**页面壳零数据，token 不进 URL**。

## 一 · AC

### AC-001 · 页面壳零数据

- `GET /` 返回 200，响应体里 **grep 项目名 / 版本号 / 任何对象名 ⇒ 0 命中**
- 判据：用 `secretproj` / 第 7 版 / 含「绝密对象」的项目起服务，壳内不得出现这些字符串

### AC-002 · 零外部依赖 + 体积

- 页面内 `<script src=` / `<link href=` / `https://` / `@import` / `integrity=` **全为 0**
- 单文件 < **200 KB**
- 判据：grep + 字节数

### AC-003 · token 不落不安全位置

- token 只存 `sessionStorage`
- 判据：`localStorage` 命中 0；URL 里不出现 token（点遍所有视图后检查地址栏/请求日志）
- 判据：页面源码里不出现 token 值

### AC-004 · 七个视图可用

| 视图 | 判据 |
| --- | --- |
| 项目与版本 | 列表与 `/api/projects` 逐项相等 |
| 评审 | 条目数、双轨状态、AI 建议与 `/api/review` 一致 |
| 导出 | 下载的 TTL/R2RML 与 CLI 产物 **sha256 相等** |
| 声明编辑 | 读到原文；保存后回执 sha256 与文件一致 |
| 审计 | 行数与 `/api/audit` 一致 |
| 查询台 | 结果表格 + 4 个元信息字段都显示 |
| 目录浏览 | 类/属性/关系数与 `/api/catalog` 一致 |

### AC-005 · 所有数据都走 REQ-124 API

- 判据：网络请求只有 `/api/*`；⛔ 不许页面直连数据库或读文件
- 判据：`console` 的 handler 里没有为前端新开「专用端点」

### AC-006 · 危险操作二次确认

- `apply` / `freeze` / 声明保存 ⇒ **必须**弹确认，确认框显示：目标（项目/版本/提案）+ 后果
- 判据：取消后**不发请求**（网络面板/日志证据）

### AC-007 · 声明保存失败保留输入

- 构造非法声明 ⇒ 保存失败，页面显示 `code` / `message` / `hint`
- 判据：**编辑器内容不被清空/回滚**（用户输入还在）

### AC-008 · 查询台必须显示元信息

- 结果区必须展示：`warnings`（可能漏数据）、`coverage`、`join_states` 里 `used=false` 的边、`truncated`
- 判据：造一个命中 `main_only` 表的查询（`deductions`）⇒ 页面**可见**警告文案
- ⛔ 只显示表格 = 不合格

### AC-009 · 错误可见

- 401/403/500/网络失败 ⇒ 页面显示可读原因（不是「加载失败」四个字）
- 判据：停掉服务后刷新页面 ⇒ 明确提示「无法连接」

### AC-010 · 无 XSS 注入

- 所有服务端返回的字符串（对象名、说明、错误信息）都用 `textContent` 渲染
- 判据：构造一个含 `<img src=x onerror=alert(1)>` 的对象名 ⇒ 页面**显示原文**，不执行
- 判据：代码里 `innerHTML` 用于用户数据的地方 = **0**

### AC-011 · 响应头

- `Content-Security-Policy`（至少 `default-src 'self'`）、`X-Content-Type-Options: nosniff`、`Referrer-Policy: no-referrer`
- 判据：`curl -I` 三个头都在

### AC-012 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` + `serve`/`query` 路由 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、新增依赖（含前端框架） |
| 基线 | REQ-124 合并后的 lib/doc 数字 |
| 出口 | 页面壳登记进 `OUTLETS`（内部轨） |

## 二 · 纪律

- **H** 页面只消费既有 API；⛔ 不为前端新开端点
- **G** 变异三元组：把 token 塞进 URL / 让壳里注入项目名 / 让查询台不显示 warnings ⇒ 每条必须被抓
- **S** AC 也会错，发现直接说
