# REQ-125 · 工作台页面的本机验证夹具

这里的东西**不是产品的一部分**，也不进构建；它们是「本机没有 headless 浏览器」时
用来给出**可复跑判据**的两件工具。

> ⚠️ **REQ-161b（2026-10-27）**：「查询台」与「目录浏览」两个视图已从页面上移除 ——
> 它们打的是 `query serve`（另一个进程）的读端点，**不属于本机 console 的本地操作**。
> 夹具因此一起收口：AC-008（查询台四段元信息）与 B-1（查询端点 404/403 ⇒
> 「查询服务未接入」）两组判据已删除（被测代码不存在了，留着只会一跑就炸）。
> 要查数据请用 Studio（`/studio`）或直接 `curl` query serve。

## 1. `page_probe.js` —— 交互层判据（Node + 假 DOM）

把 `src/console/page.rs` 里的 `<script>` **原样**抽出来，在一个最小 DOM 桩上跑：

```bash
node tests/fixtures/req125/page_probe.js
```

覆盖（**只覆盖本机 console 的本地操作**）：

| 判据 | 验什么 |
| --- | --- |
| AC-006 | 确认框取消 ⇒ **一次 fetch 都不发**；确认后才发（apply / 声明保存） |
| AC-007 | 保存失败 ⇒ 编辑器内容**未被清空 / 回滚**，且显示 `code` / `message` / `hint` |
| AC-009 | 网络失败显示「无法连接服务」+ 可读原因，不是「加载失败」 |
| AC-010 | `<img src=x onerror=alert(1)>` 只进 `textContent`；假 DOM 的 `innerHTML` 赋值即抛 |
| AC-003 | `remember()` 只写 `sessionStorage`；沙箱里没有 `localStorage` |
| **B-1 回归** | 管理端点的 401 / 403 **仍然**清口令 + 回登录态 + 显示原因（安全语义没被改坏） |
| **REQ-161b** | 构建包的门禁口径仍在（`progress.pending` / `already_imported` / `reveal_key` / `dsnFlag` / `countSummary` / 5 处 `guarded` 且都写目标+后果）；导航恰好 6 项、构建包在 02 位；**查询服务的残留 = 0**（脚本里 `sparql`/`catalog`/`describe`/`drawer`，DOM 里 `k-*`/`q-*`/`qtoken`/`#drawer`/`#v-query`/`#v-catalog`，以及 `metaLines(` / `opts.query`） |

判据条数：**45 条**（全过才 exit 0）。退出码 1 = 有判据失败。

## 2. `dev_proxy.ps1` —— 同源反向代理（本机调试，**页面已不再需要**）

页面只发**同源**请求（CSP `connect-src 'self'`）。REQ-161b 之前，「查询台 / 目录浏览」
要调的查询服务端点属于 `query serve`（:8898），与 `console serve`（:8899）是两个源，
所以需要一个按路径分流的最小代理。

**REQ-161b 之后页面只打 `console serve` 自己的 `/api/*`，直连 `:8899` 本来就是同源，
本脚本对页面不再是必需品**。它还留着，只因为 `query serve` 的 `/mcp`（MCP 客户端入口，
页面从不调用）仍可能需要经同一个源转发：

```powershell
# 需要转发 /mcp 时：两个服务先各自起好，然后
pwsh -File tests/fixtures/req125/dev_proxy.ps1
# 浏览器打开 http://127.0.0.1:8897/
```

| 路径 | 去哪个服务 |
| --- | --- |
| `/mcp` | `query serve`（默认 `127.0.0.1:8898`） |
| 其余（含 `GET /` 页面、全部 `/api/*`） | `console serve`（默认 `127.0.0.1:8899`） |

只绑回环、不做 TLS、不改写 `Authorization`。管理端点的 401/403 仍按原口径清口令回登录态
（页面侧口径见 `page_probe.js` 的「B-1 回归」那一组）。
