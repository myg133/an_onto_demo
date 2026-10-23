# REQ-125 · 工作台页面的本机验证夹具

这里的东西**不是产品的一部分**，也不进构建；它们是「本机没有 headless 浏览器」时
用来给出**可复跑判据**的两件工具。

## 1. `page_probe.js` —— 交互层判据（Node + 假 DOM）

把 `src/console/page.rs` 里的 `<script>` **原样**抽出来，在一个最小 DOM 桩上跑：

```bash
node tests/fixtures/req125/page_probe.js                      # 内置样本
node tests/fixtures/req125/page_probe.js receipt.json         # 用真实 /api/sparql 回执
```

覆盖：

| 判据 | 验什么 |
| --- | --- |
| AC-008 | `metaLines(回执)` 一定产出 warnings / coverage(main_only) / join_states(used=false) / truncated |
| AC-006 | 确认框取消 ⇒ **一次 fetch 都不发**；确认后才发（apply / 声明保存） |
| AC-007 | 保存失败 ⇒ 编辑器内容**未被清空 / 回滚**，且显示 `code` / `message` / `hint` |
| AC-009 | 网络失败显示「无法连接服务」+ 可读原因，不是「加载失败」 |
| AC-010 | `<img src=x onerror=alert(1)>` 只进 `textContent`；假 DOM 的 `innerHTML` 赋值即抛 |
| AC-003 | `remember()` 只写 `sessionStorage`；沙箱里没有 `localStorage` |
| **B-1** | 查询端点 **404 / 403** ⇒ 固定文案「查询服务未接入」（不再把用户引去管理端点清单），且 **query token 不被清**（否则直连拓扑下死循环）；查询端点 401 也不清管理口令 |
| **B-1 回归** | 管理端点的 401 / 403 **仍然**清口令 + 回登录态 + 显示原因（安全语义没被改坏） |

退出码 1 = 有判据失败。

## 2. `dev_proxy.ps1` —— 同源反向代理（本机调试）

页面只发**同源**请求（CSP `connect-src 'self'`）。`console serve`（:8899）自己提供
`/api/projects` 等控制面端点，而查询台 / 目录浏览要调的 `/api/sparql` `/api/catalog`
`/api/describe` 属于 `query serve`（:8898）。浏览器里这是两个源。

生产上由反向代理按路径分流（同一个源）；本脚本是同一口径的本机最小版：

```powershell
# 两个服务先各自起好（见 REQ-125 交付报告），然后：
pwsh -File tests/fixtures/req125/dev_proxy.ps1
# 浏览器打开 http://127.0.0.1:8897/
```

| 路径 | 去哪个服务 |
| --- | --- |
| `/api/sparql` `/api/catalog` `/api/describe` `/mcp` | `query serve`（默认 `127.0.0.1:8898`） |
| 其余（含 `GET /` 页面） | `console serve`（默认 `127.0.0.1:8899`） |

只绑回环、不做 TLS、不改写 `Authorization`。⚠️ 直连 `:8899` 时查询台会显示
「**查询服务未接入**」+ 接法（列出 query serve / 同源反代 / 本脚本），这是**刻意**的：
页面不假装加载成功，也**不会**把 404/403 说成「口令不对」（那会把用户引去改口令，
在直连拓扑下是死循环）。管理端点的 401/403 仍按原口径清口令回登录态。
