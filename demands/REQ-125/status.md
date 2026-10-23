# 状态：REQ-125 · 内部工作台前端

## 当前状态

**已完成**（2026-10-23 · 已合并 `develop` `d54c130`）

## 结果

| 项 | 值 |
| --- | --- |
| 合并 | `develop` **`d54c130`**（3 个提交）；禁改面零 diff |
| 页面 | 单文件 **59,988 B**（内联 CSS/JS），零外部依赖 |
| **壳零数据** | QA 自建 `secretproj` v1–v7 + 声明投毒「绝密对象」⇒ `GET /` 里全部 **0 命中**；响应体 sha256 **逐字节等于** `CONSOLE_HTML`（服务端无注入） |
| token | 仅 `sessionStorage`（12 处）；`localStorage`/cookie/`token=`/`location.*` 全 0 |
| **查询台元信息** | 真实回执（`deductions` main_only / 三表 `used=false` / `max_rows=1` truncated）四段逐行显示；**删掉 warnings 分支 ⇒ 探针 exit 1（3 条红）** |
| 危险操作 | apply/freeze/声明保存三处确认含「目标+后果」；取消 **0 fetch**；变异抓住 |
| 声明保存失败 | 编辑器内容**逐字保留** + `code/message/hint` |
| XSS | 只走 `textContent`；`innerHTML` 赋值 0 次 |
| 头部 | CSP / nosniff / no-referrer 齐；CSP 三点评注（为什么/代价/何时去掉） |
| 测试 | lib **590** / doc 4 / `test_req125_page` 8；OUTLETS 57（I22） |

## 两轮 QA

| 轮 | 结论 | 阻断 |
| --- | --- | --- |
| 首轮 | **FAIL** | L-1 ①：查询台在 404/403 时给**误导性**提示（列管理端点 / 清掉 query token 导致死循环）+ `--help` 未写「不含查询端点」 |
| 复审 | 修复 | 404/403 ⇒ 统一 `QUERY_NOT_WIRED` 文案、**不清任何口令**；普通 401/403 仍 `logout()`；`--help` 补三行 |

## 关键裁定

| # | 裁定 |
| --- | --- |
| L-1 | **接受「查询台需两服务同源」**（console 只管管理端点；查询台要 `query serve` + 反代）；⛔ **拒绝**让 console 自己挂查询端点（会把 query token 面带到 admin 端口）。要求失败**可见** |
| L-2 | CSP `'unsafe-inline'` 接受（单文件内联的必然代价），但必须写明「代价」与「拆外部文件时必须去掉」 |
| L-3 | 无真浏览器手测接受，但交互证据必须来自仓库内 Node 探针且可复跑；⛔ 报告不得写「已验证交互」 |
