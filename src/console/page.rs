//! REQ-125 · **内部工作台页面** —— 自包含单文件（内联 CSS/JS）、零外部依赖、壳内零数据。
//!
//! ```text
//! ontok console serve --bind 127.0.0.1:8899 --tokens admin-tokens.json \
//!     --project anspire_r2rml_excl --schema code/out/schema.json
//! → 浏览器打开 http://127.0.0.1:8899/
//! ```
//!
//! # 为什么页面与 API 同服务、但页面壳**免口令**
//!
//! 浏览器导航带不了 `Authorization` 头（与 REQ-110 / REQ-111 同一条取舍）：
//! `GET /` 必须可匿名取得，否则页面根本打不开。于是纪律是——
//!
//! | 位置 | 允许出现什么 |
//! | --- | --- |
//! | `GET /` 的响应体（本文件的 [`CONSOLE_HTML`]） | **只有**界面骨架：标题、左导航入口、表单、CSS/JS |
//! | 项目名 / 版本号 / 对象名 / 条目 / 声明原文 | **一律不出现**；全部靠拿到口令后的 `/api/*` |
//!
//! 判据（`tests/test_req125_page.rs`）：拿 `secretproj` / 第 7 版 / `绝密对象` 起服务，
//! `GET /` 的正文里 grep 这三样 ⇒ **0 命中**。
//!
//! # 口令只存 `sessionStorage`
//!
//! | 规则 | 为什么 |
//! | --- | --- |
//! | 只写 `sessionStorage` | 关掉标签页即失效；同一标签页刷新仍在（AC 要求） |
//! | ⛔ URL 参数 | 进历史 / 日志 / Referer；这也是服务端**拒绝** query string 里的 token 的理由 |
//! | ⛔ 长期存储 | 共享机器上等于长期留钥匙 |
//! | ⛔ 页面源码 | 壳是静态的，任何「示例 token」都是泄漏（连假值也不放） |
//!
//! 401 / 403 ⇒ 清掉口令 + 回登录态 + **显示服务端给的 `code` / `message` / `hint`**。
//!
//! # 六个视图消费的端点（⛔ 本文件没有、也不许有新的数据端点）
//!
//! | 视图 | 端点 |
//! | --- | --- |
//! | 项目与版本 | `GET /api/projects` · `POST /api/apply` · `POST /api/freeze` |
//! | 评审 | `GET /api/review` · `POST /api/decide` |
//! | 导出 | `GET /api/export/ttl` · `POST /api/r2rml` |
//! | 声明编辑 | `GET/PUT /api/config/{exclusions,policy,tenancy}` |
//! | 审计 | `GET /api/audit` |
//! | 构建包（REQ-159） | `GET /api/projects` · `POST /api/projects`（建项目 + 发 key，`key_label` 必填；路径作用域 `:p`，不读启动参数） · `POST /api/projects/<p>/bundles` · `PUT /api/projects/<p>/database` · `POST /api/projects/<p>/database/test` · `POST /api/projects/<p>/bundles/<id>/import`（重复调用 ⇒ `200` + `already_imported:true`，幂等） · `GET /api/review?proposal=` · `POST /api/projects/<p>/bundles/<id>/activate`（成功后固定写一句「对外服务等 P1b」） · `GET /api/projects/<p>/key`（每次查看写一条 `reveal_key` 审计） · `POST /api/projects/<p>/key/rotate`（P1a 未实现 ⇒ 404 ⇒ 显示「服务端未就绪」）。`GET /api/projects/<p>` 在 P1a **未实现，页面不调它**（不依赖）。 |
//!
//! 六组**全部**由 `console serve` 自己提供（本机 FDE 直连本机元数据库）——
//! 页面里**没有任何一个端点属于别的进程 / 别的服务**。判据：`request()` 只发
//! **同源**相对路径（`connect-src 'self'`），且没有 query token 这条旁路。
//!
//! ## REQ-161b · 为什么删掉了两个视图（不是「隐藏」，是删除）
//!
//! 三条理由（2026-10-27 与 join 对齐，⛔ 别再把它们加回来）：
//!
//! 1. **console 的定位是「本机构建工作台」**（抽取 / 评审 / 冻结 / 导出 / 打包），
//!    查数不是它的职责；
//! 2. 留一个指向 `query serve`（另一个进程）的入口 = 在 console 里放一个**跨服务半成品**：
//!    要另一把 key、另一个地址、另一套错误语义；
//! 3. FDE 要查数有 **Studio（`/studio`）**，或者直接 `curl` query serve。
//!
//! 用户的原话是「不属于我们范围的先去掉 / 我们的返回都是本地操作」。于是 REQ-161b 把
//! 这两个视图（查询台 / 目录浏览）、它们的导航入口、右侧详情抽屉、专用 CSS/JS、以及
//! 那三条只服务查询服务的错误分支 **整段删除**。
//!
//! ```text
//! 查询台 / 目录浏览 已于 2026-10-27 移除：它们打的是 query serve 的
//! 三个读端点（SPARQL 查询 / 目录分页 / 条目详情），不属于「本机构建工作台」的范围。
//! 要查数据请用 Studio（/studio）或直接 curl query serve。
//! ```
//!
//! 为什么上面那条注释里**不写那三条路径的字面量**：本单的验收口径是
//! 「本文件里 grep 那四个禁用词（小写路径词根，见派单简报 §3）= 0」。
//! 字面量点名叫在 `tests/test_req125_page.rs::ac004b_the_removed_views_leave_no_detail_panel`
//! （那里逐条断言它们**不存在**）—— 原因写在人话里，路径写在判据里，两边都不漏。
//!
//! # 构建包 tab：端点不存在时先说「服务端未就绪」，⛔ 不把它当口令错
//!
//! 构建包这一组端点（`/bundles…` / `/database…` / `/key/rotate`）在页面上线时
//! **可能还不存在**（服务端并行实现）。所以这一 tab 的每一次请求都带 `notReady`
//! 标记：HTTP **404** ⇒ 在对应区域显示一行
//! 「服务端未就绪：该端点还不存在（REQ-159 服务端在并行开发中）」，
//! 并且**不登出、不清口令** —— 404 在这里是「服务端还没这个路径」，不是「口令不对」。
//! 只有 **401 / 403** 才走既有的清口令 + 回登录态（安全语义不变）；
//! 唯一的例外是 `GET …/key` 的 **403 `key.not_admin`**：那是「口令**类型**不对」
//! （拿 query key 来看管理端点），页面只提示「需要 admin token」，**不清口令**。
//!
//! 两条只在本 tab 成立的纪律：
//!
//! | 规则 | 为什么 |
//! | --- | --- |
//! | key **按需拉取**（`GET …/key`），明文只放内存（`LAST_KEY`），⛔ 不进 sessionStorage / 输入框 | 能再次查看 ⇒ 每次查看必须留痕：服务端写一条 `reveal_key` 审计，页面把这句话**固定在按钮下面** |
//! | DSN 口令框保存成功后**只清 value**，不回显、不记录 | 页面没有回读 DSN 的端点，回显等于凭空造一份明文 |
//!
//! 四条与实际服务端（P1a，join `a127f52`）**真实行为**对齐的点 —— 别按直觉改回去：
//!
//! | 服务端行为 | 页面怎么跟 |
//! | --- | --- |
//! | 重复 import ⇒ **`200` + `already_imported:true`**（不是 409） | 当**幂等成功**渲染：版本照常算「已入库」，⛔ 不清状态、不禁用按钮、不进错误分支 |
//! | `POST …/key/rotate` 在 P1a 是 **404** | 走「服务端未就绪」（不登出），并在 `guarded()` 确认框里先写明「P1b 起可用」 |
//! | `GET /api/projects/<p>` 在 P1a **未实现** | 页面**不调**它（不依赖），只在本文件端点表登记 |
//! | `am/` 在 P1a 只被**校验**、未解析入 metadb | 组件区固定写「AM 随包收录，P1b 生效」（warn 色），⛔ 不写「已生效」；回执 `warnings[]` 里那条照原样展示 |
//!
//! 上传走 `fetch` 的**原始字节**（`Content-Type: application/octet-stream`），
//! ⛔ 不能用 `post()`：它会把 body `JSON.stringify` 成字符串，文件字节当场写坏。
//! 即便如此，整页仍然**只有一个 `fetch(` 调用点**（`request()` 里的那一处，
//! 上传只是给它传 `body` + `ctype`），`tests/test_req125_page.rs::ac004` 的数法不变。
//!
//! # XSS
//!
//! 服务端返回的每一个字符串都走 `textContent`（`el()` / `txt()` 两个助手）；
//! 页面里**没有** `innerHTML`（判据：`tests/test_req125_page.rs` grep = 0，
//! `page_probe.js` 给假 DOM 装了一个「赋值即抛」的 innerHTML 陷阱）。
//! 响应头另加 CSP（[`CSP`]）、`nosniff`、`no-referrer`（[`SECURITY_HEADERS`]）。
//!
//! # 大结果集
//!
//! 评审 40 条/页、审计默认 100（服务端上限 1000，页面最多渲染 1000）。⛔ 不一次渲染上万行。
//!
//! # 壳（REQ-161 换皮 / REQ-161b 收口）：左导航 + 单一主区、32px 行高 / sticky 表头、key·IRI 等宽
//!
//! | 结构 | 判据 |
//! | --- | --- |
//! | 左导航 `#nav` 里 6 个 `.nav-item`（REQ-161b：查询台 / 目录浏览已移除）；顶栏 `#tabs` 不再存在 | 按钮 id 仍是 `tab-*`，`switchTab` 的口径没变 |
//! | 顺序：01 项目与版本 / 02 构建包 / 03 评审 / 04 导出 / 05 声明编辑 / 06 审计 | 构建包是用户每天第一件事（上传包），所以在第 2 位 |
//! | 单一主区 `#views` 是**唯一**滚动容器 | 视图容器 `v-*` 一个不少 |
//! | `thead` sticky + `tbody td` 行高 32px | 表体在 `.tablewrap` 里滚动（审计表是最长的那张） |
//! | key / IRI 一律等宽 `.key`（审计 / 评审 / 项目三处） | CSS 有定义 + 三处确实在用 |
//! | 构建包是**本机主入口**，ADR-013 只在视图底部留一行中性说明 | ⛔ 不再写「仅为历史保留 / 请在 Studio 里做」 |
//!
//! ⛔ 换皮不许改的事实：`/api/*` 路径集合（REQ-161b 之后 9 条，见
//! `the_api_path_set_is_frozen`），壳内仍然零数据、零外部资源。

use axum::http::{HeaderName, HeaderValue};

/// 页面壳（**零数据**：任何项目名 / 版本号 / 对象名都不在这里）。
///
/// ⛔ 改这个常量时守住四件事：
/// 1. 不出现 `src=` / `href=` 的外部资源、不出现 `@import`；
/// 2. 不出现任何真实数据（项目名、版本号、对象名、表名、token）；
/// 3. 用户数据一律 `textContent`（`el` / `txt`），**没有** `innerHTML`；
/// 4. 口令只进 `sessionStorage`。
pub const CONSOLE_HTML: &str = r####"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>内部工作台</title>
<style>
:root{color-scheme:dark;--bg:#070a10;--bg-2:#0b0f17;--card:#111721;--head:#0d131c;--line:rgba(255,255,255,.085);--line-2:rgba(255,255,255,.045);--fg:#e8edf6;--mut:#9aa7bd;--mut-2:#64708a;--acc:#6ec8ff;--ok:#5fd3a3;--warn:#e6b455;--bad:#ef7a8a;--mono:ui-monospace,"Cascadia Mono","JetBrains Mono",Consolas,monospace;--row:32px;--ease:cubic-bezier(.22,.61,.36,1)}
*{box-sizing:border-box}
[hidden]{display:none!important}
html,body{height:100%}
body{margin:0;background:var(--bg);color:var(--fg);font:13px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;-webkit-font-smoothing:antialiased}
/* ── 壳：左侧窄导航 + 单一主区 ─────────────────────────── */
.app{height:100%;display:grid;grid-template-columns:212px minmax(0,1fr)}
#nav{display:flex;flex-direction:column;min-height:0;border-right:1px solid var(--line);background:linear-gradient(180deg,rgba(255,255,255,.022),transparent 42%)}
.brand{display:flex;align-items:center;gap:9px;padding:13px 13px 11px;border-bottom:1px solid var(--line-2)}
.brand-mark{width:17px;height:17px;border-radius:50%;flex:none;background:radial-gradient(circle at 34% 30%,#dff1ff,#6ec8ff 42%,#1b4f77 78%);box-shadow:0 0 12px rgba(110,200,255,.45)}
.brand-text{font-weight:600;font-size:13px;letter-spacing:.2px}
.brand-text em{display:block;font-style:normal;font-weight:400;font-size:10px;color:var(--mut-2);letter-spacing:.14em;text-transform:uppercase;margin-top:1px}
#nav-list{flex:1;min-height:0;overflow:auto;padding:8px;display:flex;flex-direction:column;gap:2px}
.nav-group{font-size:10px;letter-spacing:.12em;text-transform:uppercase;color:var(--mut-2);opacity:.8;padding:9px 9px 3px}
.nav-group:first-child{padding-top:2px}
.nav-item{display:flex;align-items:center;gap:8px;height:auto;padding:7px 9px;border:0;border-radius:8px;background:transparent;color:var(--mut);font:inherit;text-align:left;cursor:pointer;transition:background .16s var(--ease),color .16s var(--ease)}
.nav-item:hover{background:rgba(255,255,255,.028);color:var(--fg)}
.nav-item.is-on{background:rgba(255,255,255,.05);color:var(--fg);box-shadow:inset 2px 0 0 var(--acc)}
.nav-i{font:10px/1 var(--mono);color:var(--mut-2)}
.nav-item.is-on .nav-i{color:var(--acc)}
#nav-foot{padding:9px 12px 11px;border-top:1px solid var(--line-2);display:grid;gap:4px}
#nav-foot .kv{display:flex;justify-content:space-between;gap:8px;font-size:11.5px;color:var(--mut-2)}
#who{color:var(--mut);font-weight:600;font-size:11.5px}
.nav-note{font-size:10.5px;color:var(--mut-2);line-height:1.45}
#main{min-width:0;min-height:0;display:flex;flex-direction:column}
#topbar{display:flex;align-items:center;gap:10px;padding:8px 14px;border-bottom:1px solid var(--line);background:rgba(11,15,23,.72);flex-wrap:wrap}
.ctx-label{font-size:11.5px;color:var(--mut-2)}
#login{display:flex;gap:8px;align-items:center;flex-wrap:wrap}
#msg{padding:7px 14px;border-bottom:1px solid var(--line-2);font-size:12px;color:var(--mut);min-height:32px}
#msg.bad{color:var(--bad);font-weight:600}
#msg.ok{color:var(--ok)}
#views{flex:1;min-height:0;overflow:auto;padding:14px 16px 64px}
/* ── 视图容器 ─────────────────────────────────────────── */
.view[hidden]{display:none}
.bar{display:flex;gap:10px;align-items:center;flex-wrap:wrap;margin-bottom:10px}
.note{color:var(--mut);font-size:12px}
.summary{color:var(--mut);font-size:12px;margin-bottom:10px}
.summary b{color:var(--fg)}
/* ── 面板 ─────────────────────────────────────────────── */
.panel{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:10px 12px;margin-bottom:12px}
.panel h2{font-size:12.5px;margin:0 0 8px;color:var(--fg);font-weight:600}
.panel h3{font-size:12px;margin:10px 0 6px;color:var(--mut);font-weight:600}
.grid2{display:grid;grid-template-columns:1fr 1fr;gap:12px}
@media (max-width:900px){.grid2{grid-template-columns:1fr}}
/* ── 表格 / 表单 / 按钮（密度见检查点 2） ─────────────── */
table{width:100%;border-collapse:collapse;font-size:13px}
th,td{text-align:left;padding:4px 8px;border-bottom:1px solid var(--line);vertical-align:top}
th{font-weight:600;font-size:12px;color:var(--mut);white-space:nowrap}
td.mono,.mono{font-family:var(--mono);font-size:12px;word-break:break-all}
input,select,textarea{font:inherit;color:var(--fg);background:rgba(255,255,255,.03);border:1px solid var(--line);border-radius:7px;padding:3px 8px;height:28px;outline:none;transition:border-color .16s var(--ease),background .16s var(--ease)}
input:focus,select:focus,textarea:focus{border-color:rgba(110,200,255,.5);background:rgba(255,255,255,.05)}
input::placeholder{color:var(--mut-2)}
input[type=password]{width:200px;font:12px/1 var(--mono)}
select option{background:var(--bg-2);color:var(--fg)}
textarea{width:100%;height:auto;min-height:260px;font-family:var(--mono);font-size:12px;line-height:1.6;padding:10px;resize:vertical}
button{font:inherit;height:28px;padding:3px 10px;border:1px solid var(--line);border-radius:7px;background:rgba(255,255,255,.03);color:var(--mut);cursor:pointer;white-space:nowrap;transition:background .16s var(--ease),border-color .16s var(--ease),color .16s var(--ease)}
button:hover{border-color:rgba(110,200,255,.5);color:var(--acc)}
button:disabled{opacity:.45;cursor:default}
button.danger{border-color:rgba(239,122,138,.4);color:var(--bad);background:rgba(239,122,138,.07)}
button.danger:hover{background:rgba(239,122,138,.16);color:#ffc7ce}
button.ok{border-color:rgba(95,211,163,.4);color:var(--ok);background:rgba(95,211,163,.07)}
/* ── 状态 / 提示 ─────────────────────────────────────── */
.badge{display:inline-block;font-size:11.5px;padding:0 7px;border-radius:999px;border:1px solid var(--line);color:var(--mut);white-space:nowrap}
.badge.ok{color:var(--ok);border-color:rgba(95,211,163,.35);background:rgba(95,211,163,.08)}
.badge.bad{color:var(--bad);border-color:rgba(239,122,138,.35);background:rgba(239,122,138,.08)}
.badge.warn{color:var(--warn);border-color:rgba(230,180,85,.34);background:rgba(230,180,85,.08)}
.badge.acc{color:var(--acc);border-color:rgba(110,200,255,.35);background:rgba(110,200,255,.09)}
ul.warns{margin:6px 0;padding-left:18px}
ul.warns li{margin:3px 0;color:var(--warn)}
ul.warns li.bad{color:var(--bad);font-weight:600}
article.item{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:8px 11px;margin-bottom:8px}
article.item.vetoed{border-color:var(--bad)}
.hd{display:flex;gap:9px;align-items:baseline;flex-wrap:wrap}
.idx{color:var(--mut-2);font-size:12px;font-variant-numeric:tabular-nums;font-family:var(--mono)}
.target{font-weight:600}
.kind{font-size:11.5px;color:var(--mut);border:1px solid var(--line);border-radius:999px;padding:0 8px}
.why{font-size:12px;color:var(--mut);margin-top:4px}
.why b{color:var(--fg)}
.tracks{display:flex;gap:18px;flex-wrap:wrap;margin-top:6px;font-size:13px}
.track{display:flex;gap:6px;align-items:center}
.track .name{color:var(--mut)}
.st{font-size:11.5px;border:1px solid var(--line);border-radius:5px;padding:0 6px}
.st.accepted{color:var(--ok);border-color:var(--ok)}
.st.rejected{color:var(--bad);border-color:var(--bad)}
.st.pending{color:var(--mut)}
pre{margin:6px 0 0;padding:8px;background:rgba(0,0,0,.28);border:1px solid var(--line);border-radius:6px;font-size:12px;overflow:auto;max-height:300px;white-space:pre-wrap;word-break:break-all}
.pager{display:flex;gap:10px;align-items:center;margin-top:10px;font-size:12px;color:var(--mut)}
.err{background:var(--card);border:1px solid var(--bad);border-radius:10px;padding:10px 12px;margin:10px 0}
.err .code{font-family:var(--mono);font-size:12px;color:var(--bad);font-weight:600}
.err .msg{margin-top:4px}
.err .hint{margin-top:4px;color:var(--mut);font-size:12px}
.okbox{background:var(--card);border:1px solid var(--ok);border-radius:10px;padding:10px 12px;margin:10px 0}
.kv{display:grid;grid-template-columns:160px 1fr;gap:2px 12px;font-size:12.5px}
.kv dt{color:var(--mut)}
.kv dd{margin:0;word-break:break-all}
.empty{color:var(--mut-2);text-align:center;padding:26px 0}
/* ── 弹窗（危险动作确认：目标 + 后果） ────────────────── */
#modal{position:fixed;inset:0;background:rgba(0,0,0,.62);display:flex;align-items:center;justify-content:center;padding:20px;z-index:30}
#modal[hidden]{display:none}
#mbox{background:var(--card);border:1px solid var(--line);border-radius:12px;max-width:640px;width:100%;max-height:82vh;overflow:auto;padding:16px 20px}
#mbox h2{margin:0 0 10px;font-size:16px}
#m-rows{display:grid;grid-template-columns:72px 1fr;gap:6px 10px;margin:0 0 12px;font-size:12.5px}
#m-rows dt{color:var(--mut)}
#m-rows dd{margin:0;word-break:break-all}
#mbox .acts{display:flex;gap:10px;justify-content:flex-end;margin-top:6px}
/* ── 视图头 / 分块面板（检查点 2：信息密度） ─────────── */
.view-head{margin:0 0 12px;max-width:980px}
.view-head h2{margin:0 0 4px;font-size:15px;font-weight:600;letter-spacing:.2px}
.view-head p{margin:0;color:var(--mut-2);font-size:12px}
.view-head strong{color:var(--mut);font-weight:600}
code{font:11.5px/1 var(--mono);color:var(--acc);background:rgba(110,200,255,.09);padding:1px 4px;border-radius:4px}
.panel-flush{padding:0;overflow:hidden}
.panel-head{display:flex;align-items:center;gap:8px;padding:7px 11px;border-bottom:1px solid var(--line-2);font-size:12px;color:var(--mut);background:rgba(255,255,255,.018);flex-wrap:wrap}
.panel-body{padding:11px}
.panel-body>*:last-child{margin-bottom:0}
.panel-head>h2{font-size:12px;margin:0;color:var(--fg);font-weight:600}
.spacer{margin-left:auto}
label{display:inline-flex;gap:6px;align-items:center;color:var(--mut-2);font-size:11.5px}
/* ── 表格：行高 32px + 表头 sticky + key/IRI 等宽 ─────── */
.tablewrap{overflow:auto;max-height:calc(100vh - 300px);border:1px solid var(--line-2);border-radius:8px}
.tablewrap.tall{max-height:calc(100vh - 250px)}
table{width:100%;border-collapse:separate;border-spacing:0;font-size:12.5px}
thead th{position:sticky;top:0;z-index:1;text-align:left;font-weight:500;color:var(--mut-2);font-size:11.5px;height:30px;padding:0 10px;background:var(--bg-2);border-bottom:1px solid var(--line);white-space:nowrap}
tbody td{height:32px;padding:0 10px;border-bottom:1px solid var(--line-2);color:var(--mut);vertical-align:middle;white-space:nowrap}
tbody tr:hover td{background:rgba(255,255,255,.026);color:var(--fg)}
td.mono,.mono{font-family:var(--mono);font-size:11.5px;word-break:break-all}
.key{font-family:var(--mono);font-size:12px;color:#cfe6ff;word-break:break-all}
/* ── 校验清单（声明编辑：把「保存前校验」说清） ──────── */
.checks{display:grid;gap:7px;font-size:12px}
.chk{display:flex;gap:8px;align-items:flex-start;color:var(--mut)}
.chk::before{content:"○";color:var(--mut-2);flex:none}
.chk.ok::before{content:"✓";color:var(--ok)}
.chk.bad::before{content:"✕";color:var(--bad)}
.chk .why{color:var(--mut-2)}
/* ── 滚动条 ───────────────────────────────────────────── */
::-webkit-scrollbar{width:10px;height:10px}
::-webkit-scrollbar-track{background:transparent}
::-webkit-scrollbar-thumb{background:rgba(255,255,255,.09);border-radius:6px;border:2px solid transparent;background-clip:content-box}
::-webkit-scrollbar-thumb:hover{background:rgba(255,255,255,.16);background-clip:content-box}
@media (max-width:1080px){.app{grid-template-columns:64px minmax(0,1fr)}.brand-text,.nav-foot,.nav-group,.nav-i{display:none}.nav-item{justify-content:center;padding:9px 4px}.grid2{grid-template-columns:1fr}}
</style>
</head>
<body>
<div class="app">
  <aside id="nav" aria-label="视图导航">
    <div class="brand">
      <span class="brand-mark" aria-hidden="true"></span>
      <span class="brand-text">内部工作台<em>Console</em></span>
    </div>
    <nav id="nav-list">
      <div class="nav-group">建库</div>
      <button id="tab-projects" class="nav-item is-on" data-tab="projects"><span class="nav-i">01</span>项目与版本</button>
      <div class="nav-group">入库</div>
      <button id="tab-bundles" class="nav-item" data-tab="bundles"><span class="nav-i">02</span>构建包</button>
      <div class="nav-group">签与交付</div>
      <button id="tab-review" class="nav-item" data-tab="review"><span class="nav-i">03</span>评审</button>
      <button id="tab-export" class="nav-item" data-tab="export"><span class="nav-i">04</span>导出</button>
      <button id="tab-config" class="nav-item" data-tab="config"><span class="nav-i">05</span>声明编辑</button>
      <button id="tab-audit" class="nav-item" data-tab="audit"><span class="nav-i">06</span>审计</button>
    </nav>
    <div id="nav-foot">
      <div class="kv"><span>连接</span><b id="who">未连接</b></div>
      <div class="nav-note">口令只在本标签页的 sessionStorage</div>
    </div>
  </aside>

  <main id="main">
    <header id="topbar">
      <span class="ctx-label">管理口令</span>
      <span id="login">
        <input id="token" type="password" placeholder="管理口令（admin token）" autocomplete="off">
        <button id="go">载入</button>
      </span>
    </header>
    <div id="msg">口令只留在本标签页的 sessionStorage 里：不进地址栏、不写长期存储。</div>

    <div id="views">  <!-- ── 1 · 项目与版本 ─────────────────────────────────── -->
  <section id="v-projects" class="view">
    <div class="view-head">
      <h2>项目与版本</h2>
      <p>版本是<strong>不可变</strong>的：冻结后不能改，只能出新版本；导出与查询都只认已冻结版本。</p>
    </div>
    <div class="panel panel-flush">
      <div class="panel-head">
        <span>版本与冻结状态</span>
        <span class="spacer"></span>
        <span class="mono">GET /api/projects</span>
        <button id="p-refresh">刷新</button>
      </div>
      <div class="panel-body">
        <div id="p-summary" class="summary">尚未载入。</div>
        <div id="p-table" class="tablewrap"></div>
      </div>
    </div>
    <div class="panel panel-flush">
      <div class="panel-head"><span>应用提案（apply）</span><span class="spacer"></span><span class="mono">POST /api/apply</span></div>
      <div class="panel-body">
        <p class="note">apply 会把审核过的提案写成<strong>新版本</strong>（对象 / 关系落库）。已应用的提案再 apply ⇒ 409；基版本已冻结也 ⇒ 409。此动作不可撤销。</p>
        <div class="bar">
          <label>提案别名 <input id="p-alias" value="p1" size="10"></label>
          <button id="p-apply" class="danger">应用提案…</button>
        </div>
        <div id="p-apply-out"></div>
      </div>
    </div>
  </section>

  <!-- ── 3 · 评审 ───────────────────────────────────────── -->
  <section id="v-review" class="view" hidden>
    <div class="view-head">
      <h2>评审</h2>
      <p>AI 出的提案逐条过，<strong>两轨都要签</strong>（业务轨 + 建模轨）；决策记在审计里，改不回来只能再提一轮。</p>
    </div>
    <div class="panel panel-flush">
      <div class="panel-head">
        <label>提案 <input id="r-proposal" value="p1" size="8"></label>
        <button id="r-load">载入</button>
        <span class="spacer"></span>
        <span class="mono">GET /api/review?proposal=… · POST /api/decide</span>
      </div>
      <div class="panel-body">
        <div id="r-summary" class="summary">尚未载入。</div>
        <div id="r-list"></div>
        <div class="pager" id="r-pager"></div>
      </div>
    </div>
  </section>

  <!-- ── 4 · 导出 ───────────────────────────────────────── -->
  <section id="v-export" class="view" hidden>
    <div class="view-head">
      <h2>导出</h2>
      <p>客户视图与建模视图<strong>不是同一份</strong>；页面不加工正文，<strong>字节与 CLI 产物逐字节相同</strong>（sha256 可对拍）。</p>
    </div>
    <div class="grid2">
      <div class="panel panel-flush">
        <div class="panel-head"><span>TTL</span><span class="spacer"></span><span class="mono">GET /api/export/ttl</span></div>
        <div class="panel-body">
          <p class="note">与 <code>ontok ontology export</code> 是同一份字节（sha256 应相等）。</p>
          <div class="bar">
            <label>版本 <select id="e-version"></select></label>
            <label><input type="checkbox" id="e-model"> 建模视图（model_view=1）</label>
          </div>
          <div class="bar">
            <label>schema <input id="e-schema" placeholder="留空 = 服务端默认" size="26"></label>
          </div>
          <div class="bar">
            <label>tenancy <input id="e-tenancy" placeholder="留空 = 不传" size="26"></label>
          </div>
          <div class="bar"><button id="e-ttl">导出 TTL</button></div>
          <div id="e-ttl-out"></div>
        </div>
      </div>
      <div class="panel panel-flush">
        <div class="panel-head"><span>R2RML</span><span class="spacer"></span><span class="mono">POST /api/r2rml</span></div>
        <div class="panel-body">
          <p class="note">与 <code>ontok r2rml generate</code> 是同一份字节；<code>include_inherit</code> 决定继承边要不要生成 join。</p>
          <div class="bar">
            <label>版本 <select id="e2-version"></select></label>
            <label><input type="checkbox" id="e2-inherit"> include_inherit</label>
          </div>
          <div class="bar">
            <label>schema（必填，服务端路径） <input id="e2-schema" placeholder="如 code/out/schema.json" size="26"></label>
          </div>
          <div class="bar">
            <label>排除文件（可选） <input id="e2-exclude" placeholder="服务端路径" size="26"></label>
          </div>
          <div class="bar"><button id="e2-go">生成 R2RML</button></div>
          <div id="e2-out"></div>
        </div>
      </div>
    </div>
  </section>

  <!-- ── 5 · 声明编辑 ───────────────────────────────────── -->
  <section id="v-config" class="view" hidden>
    <div class="view-head">
      <h2>声明编辑</h2>
      <p>保存前<strong>按真实 schema 校验</strong>表 / 列：<code>main_only</code> 与 <code>publicDimensions</code> 必须带 <code>reason</code>；校验失败 ⇒ 原文件一个字节都不变。</p>
    </div>
    <div class="panel panel-flush">
      <div class="panel-head">
        <label>声明
          <select id="c-kind">
            <option value="exclusions">exclusions（排除）</option>
            <option value="policy">policy（租户口径）</option>
            <option value="tenancy">tenancy（租户列标注）</option>
          </select>
        </label>
        <button id="c-load">载入</button>
        <button id="c-save" class="danger">保存…</button>
        <span class="spacer"></span>
        <span class="mono">GET / PUT /api/config/…</span>
      </div>
      <div class="panel-body">
        <div id="c-meta" class="note"></div>
        <textarea id="c-text" spellcheck="false" placeholder="点「载入」读取当前声明原文；保存失败时这里的内容不会被清空或回滚。"></textarea>
        <div id="c-out"></div>
      </div>
    </div>
    <div class="panel panel-flush">
      <div class="panel-head"><span>保存前会发生什么（校验在后端，失败一个字节都不落盘）</span></div>
      <div class="panel-body">
        <div class="checks">
          <div class="chk"><span>表 / 列必须存在</span><span class="why">— 服务端按 schema 校验，对不上 ⇒ 400</span></div>
          <div class="chk"><span><code>main_only</code> / <code>publicDimensions</code> 必须带 <code>reason</code></span><span class="why">— 缺 reason ⇒ 400</span></div>
          <div class="chk"><span>PUT 是整份替换</span><span class="why">— 校验通过才落盘，失败时编辑器内容原样保留</span></div>
        </div>
      </div>
    </div>
  </section>

  <!-- ── 6 · 审计 ───────────────────────────────────────── -->
  <section id="v-audit" class="view" hidden>
    <div class="view-head">
      <h2>审计</h2>
      <p>每个写操作一行，<strong>含失败</strong>；令牌只记 <code>sha256</code> 前 12 位——不记原文。</p>
    </div>
    <div class="panel panel-flush">
      <div class="panel-head">
        <label>最近 <input id="a-limit" value="100" size="5"> 条</label>
        <label>动作
          <select id="a-action">
            <option value="">全部</option>
            <option value="import">import</option>
            <option value="apply">apply</option>
            <option value="freeze">freeze</option>
            <option value="export">export</option>
            <option value="r2rml">r2rml</option>
            <option value="config">config</option>
            <option value="decide">decide</option>
            <option value="auth">auth</option>
          </select>
        </label>
        <button id="a-load">刷新</button>
        <span class="spacer"></span>
        <span class="mono">GET /api/audit?limit=（上限 1000，oldest_first）</span>
      </div>
      <div class="panel-body">
        <div id="a-summary" class="summary">尚未载入。</div>
        <div id="a-table" class="tablewrap tall"></div>
        <div class="note">筛选在页面里做，只影响显示，不影响服务端返回。</div>
      </div>
    </div>
  </section>

  <!--
    查询台 / 目录浏览 已于 2026-10-27 移除（REQ-161b）：它们打的是 query serve 的
    三个读端点（SPARQL 查询 / 目录分页 / 条目详情），不属于「本机构建工作台」的范围。
    要查数据请用 Studio（/studio）或直接 curl query serve。
    ⛔ 别加回来：留一个指向另一个进程的入口 = 在 console 里放一个跨服务半成品
    （要另一把 key、另一个地址、另一套错误语义）。理由详见本文件头部 §REQ-161b。
  -->

  <!-- ── 2 · 构建包（REQ-159）· 本机 FDE 主入口 ─────────── -->
  <section id="v-bundles" class="view" hidden>
    <div class="view-head">
      <h2>构建包</h2>
      <p>本机 FDE 操作：建项目 → 上传包 → 配目标库（需要时）→ 解析入库 → 待确认 → 生效。<strong>全部走本机 console</strong>（本地元数据库），不依赖任何外部服务。</p>
    </div>
    <div class="bar">
      <button id="b-refresh">刷新</button>
      <span class="note">当前项目：</span><span id="b-project">—</span>
      <span class="note">（GET /api/projects 报的是服务端启动时配置的那个项目；本页新建的项目会临时接管这里）</span>
    </div>
    <div id="b-msg"></div>

    <div class="panel panel-flush">
      <div class="panel-head"><span>1 · 上传并校验（POST …/bundles，application/octet-stream）</span></div>
      <div class="panel-body">
        <p class="note">包是构建端产出的 .ontokbundle 文件；页面只把<strong>文件字节原样</strong> POST 上去（⛔ 不套 JSON），服务端校验 sha256 与组件清单后回 state / bundle_id。</p>
        <div class="bar">
          <label>选包 <input id="b-file" type="file" accept=".ontokbundle"></label>
          <button id="b-upload">上传并校验</button>
        </div>
        <div id="b-state" class="summary">还没有上传。</div>
        <div id="b-components"></div>

        <details style="margin-top:12px">
          <summary class="note">新建项目（POST /api/projects：建项目 + 生成 key；默认折叠）</summary>
          <div class="bar" style="margin-top:8px">
            <label>项目名 <input id="b-new-project" size="18" placeholder="小写字母/数字/连字符/下划线" autocomplete="off"></label>
            <label>备注 <input id="b-new-label" size="16" placeholder="可选" autocomplete="off"></label>
            <label>key 用途 <input id="b-new-key-label" size="18" placeholder="必填：谁在用这把 key" autocomplete="off"></label>
            <button id="b-new-submit">建立项目并生成 key</button>
          </div>
          <p class="note">项目名只许小写字母 / 数字 / 连字符 / 下划线（首字符字母或数字，2–63 位；大写、空格、斜杠、点都不行）；<code>key_label</code> 必填 —— 审计要回答「这把 key 是谁在用」（⛔ 别把密钥本身填进去）。建好的项目立刻可用于本 tab 的上传 / 解析 / 激活 / 查看 key：这些端点按路径 <code>:p</code> 作用域，跟服务端启动参数无关。</p>
          <div id="b-new-out"></div>
        </details>
      </div>
    </div>

    <div id="b-dsn" class="panel panel-flush" hidden>
      <div class="panel-head"><span>2 · 目标库连接（requires_dsn = true 时才需要）</span></div>
      <div class="panel-body">
        <p class="note">DSN 由本页按 postgresql 形式拼（user / password 走 encodeURIComponent）；口令<strong>只在这一次 PUT 里出现</strong>，保存成功后只清空口令框：不回显、不记录、不进 sessionStorage。</p>
        <div class="bar">
          <label>host <input id="b-dsn-host" size="18" placeholder="数据库主机"></label>
          <label>port <input id="b-dsn-port" size="6" value="5432"></label>
          <label>库 <input id="b-dsn-db" size="14" placeholder="库名"></label>
        </div>
        <div class="bar">
          <label>user <input id="b-dsn-user" size="14" autocomplete="off"></label>
          <label>password <input id="b-dsn-pass" type="password" autocomplete="new-password"></label>
          <label>sslmode
            <select id="b-dsn-ssl">
              <option value="disable">disable</option>
              <option value="require">require</option>
              <option value="prefer">prefer</option>
            </select>
          </label>
        </div>
        <div class="bar">
          <button id="b-dsn-save">保存 DSN</button>
          <button id="b-dsn-test">测试连通</button>
          <span class="note">PUT …/database 的回执只有 database_id / host / database / user（⛔ 响应里没有 dsn，本页也不显示）；测试打的是服务端<strong>已保存</strong>的那份连接。</span>
        </div>
        <div id="b-dsn-out"></div>
      </div>
    </div>

    <div class="panel panel-flush">
      <div class="panel-head"><span>3 · 解析入库（POST …/bundles/&lt;id&gt;/import）</span></div>
      <div class="panel-body">
        <div class="bar">
          <button id="b-import" disabled>解析入库</button>
          <span class="note">条件没满足时按钮是灰的：先上传拿到 bundle_id；requires_dsn 的包还要 DSN 保存 + 连通测试通过。</span>
        </div>
        <div id="b-import-out"></div>
      </div>
    </div>

    <div class="panel panel-flush">
      <div class="panel-head"><span>4 · 待确认清单（GET /api/review?proposal=p1）</span></div>
      <div class="panel-body">
        <p class="note">本 tab 只做<strong>汇总与门禁</strong>：逐条裁决仍在「评审」tab 做（⛔ 这里不调 /api/decide —— 两套裁决入口等于两套口径）。待确认条数取服务端的 <code>progress.pending</code>（= total − signed，两轨都定了才算签完）、阻断条数取 <code>progress.vetoed</code>；只有<strong>待确认 0 条</strong>时「冻结并切流量」才可点。</p>
        <div class="bar"><button id="b-checklist-refresh">刷新清单</button></div>
        <div id="b-checklist" class="summary">尚未载入。</div>
      </div>
    </div>

    <div class="panel panel-flush">
      <div class="panel-head"><span>5 · 生效（冻结并切流量）· key</span></div>
      <div class="panel-body">
        <p class="note">激活 = 把这一版冻结、并让查询服务切过去；不可撤销。key 可以<strong>随时再次查看</strong>（<code>GET …/key</code>），本页只在内存里放着这一次拿到的明文供复制，不落任何浏览器存储。</p>
        <div class="bar">
          <button id="b-activate" class="danger" disabled>冻结并切流量…</button>
          <button id="b-rotate" class="danger">换 key…</button>
          <button id="b-key-view">查看 key</button>
        </div>
        <div class="note">本次查看会记一条审计（action=reveal_key）—— 这是「能再次查看」的唯一补偿，别把它删掉。</div>
        <div id="b-key" hidden>
          <div class="note">key 明文（按需拉取；只留在本页内存里，供复制）：</div>
          <div id="b-key-val" class="mono"></div>
          <div id="b-key-meta" class="note"></div>
          <div class="bar"><button id="b-key-copy">复制 key</button></div>
        </div>
        <div id="b-key-out"></div>
      </div>
    </div>
    <p class="note">客户侧平台（Studio）另有同一套流程（ADR-013）；本页是 FDE 本机入口。</p>
  </section>
  </div>
  </main>
</div>

<div id="modal" hidden>
  <div id="mbox" role="dialog" aria-modal="true">
    <h2 id="m-title"></h2>
    <dl id="m-rows"></dl>
    <p class="note" id="m-note">取消不会发出任何请求。</p>
    <div class="acts">
      <button id="m-cancel">取消（不发请求）</button>
      <button id="m-ok" class="danger">确认执行</button>
    </div>
  </div>
</div>

<script>
"use strict";
// ══════════════════════════════════════════════════════════
// 状态：口令只在内存 + sessionStorage（键名固定，刷新后同一标签页可恢复）
// ══════════════════════════════════════════════════════════
var ADMIN_KEY = "ontok.console.admin";
var PAGE_SIZE_REVIEW = 40;

var S = {
  at: "", connected: false, tab: "projects",
  projects: null, review: null, reviewProposal: "", reviewPage: 1,
  declKind: "exclusions", audit: null,
  // 构建包（REQ-159）：bundle = 上传回执 + 入库后补的 version；
  // pendingCount = 待确认条数（来源：/api/review 的 progress.pending，缺 progress 时按条目估算）；
  // projectOverride = 本页新建的项目名（非空时它就是「当前项目」，别被 GET /api/projects 冲掉）
  bundle: null, needDsn: false, dsnOk: false, checklist: null, pendingCount: null,
  projectOverride: ""
};

function $(id) { return document.getElementById(id); }
function el(tag, cls, text) {
  var n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined && text !== null) n.textContent = String(text);
  return n;
}
function txt(node, s) { node.textContent = (s === undefined || s === null) ? "" : String(s); }
function clear(node) { node.textContent = ""; }
function setMsg(s, cls) { var m = $("msg"); txt(m, s); m.className = cls || ""; }
function badge(text, cls) { return el("span", "badge " + (cls || ""), text); }
function shortSha(s) { return s ? (String(s).slice(0, 12) + "…") : "—"; }
function fmtBytes(n) { return (typeof n === "number") ? (n + " 字节") : "—"; }
function yesNo(v, yes, no) { return v ? (yes || "是") : (no || "否"); }

// ══════════════════════════════════════════════════════════
// 请求层：只发同源 /api/*，只带 Authorization 头
// ══════════════════════════════════════════════════════════
function wbError(status, code, message, hint) {
  var e = new Error(message);
  e.wbError = true; e.status = status; e.code = code; e.message = message; e.hint = hint;
  return e;
}
function httpError(status, raw) {
  var body = null;
  try { body = raw ? JSON.parse(raw) : null; } catch (e) { body = null; }
  var code = (body && (body.code || body.error)) || ("HTTP_" + status);
  var message = (body && (body.message || body.error)) || (raw ? raw.slice(0, 300) : ("HTTP " + status));
  var hint = (body && body.hint) || "";
  return wbError(status, String(code), String(message), String(hint));
}
function netError(e) {
  var why = (e && e.message) ? e.message : String(e);
  return wbError(0, "NETWORK",
    "无法连接服务：" + why,
    "确认服务还在跑（端口 / 进程）；本页只允许同源 /api/*（CSP connect-src 'self'）。");
}
function authHeader() {
  return S.at ? { "Authorization": "Bearer " + S.at } : {};
}
// 构建包 tab 的端点（REQ-159）可能还没上线：这类请求带 `notReady`，404 说同一句话。
// **把「服务端没有这个路径」说成「口令不对」是死循环**（REQ-125 · L-1 那条教训）。
var NOT_READY_MSG = "服务端未就绪：该端点还不存在（REQ-159 服务端在并行开发中）";
var NOT_READY_HINT = "这不是口令问题：页面**不会**清空口令、也不会退回登录态。" +
  "等服务端把这一组端点上线后重试；在那之前本页其余五个视图照常可用。";
function request(path, opts) {
  opts = opts || {};
  var headers = authHeader();
  if (opts.json) headers["Content-Type"] = "application/json";
  // 上传：body 是文件句柄本身（原始字节），内容类型由调用点指定 —— 仍然走下面这**唯一**一处 fetch。
  if (opts.ctype) headers["Content-Type"] = opts.ctype;
  var init = { method: opts.method || "GET", headers: headers, cache: "no-store" };
  if (opts.body !== undefined) init.body = opts.body;
  return fetch(path, init).then(function (r) {
    return r.text().then(function (raw) {
      if (r.ok) return { status: r.status, text: raw, ctype: r.headers.get("content-type") || "" };
      var err = httpError(r.status, raw);
      if (opts.notReady && r.status === 404) {
        // ⛔ 不登出：404 = 服务端还没这个路径，不是口令错（清口令只会把人挡在门外）。
        err.code = "ENDPOINT_NOT_READY";
        err.message = NOT_READY_MSG;
        err.hint = NOT_READY_HINT;
      } else if (opts.needAdmin && r.status === 403) {
        // 403 key.not_admin = 「口令**类型**不对」（拿 query key 看管理端点），不是口令过期。
        // ⇒ 只说清该用 admin token，⛔ 不清口令、不登出（与 404 那条同一条教训）。
        err.code = "KEY_NOT_ADMIN";
        err.message = "需要 admin token（key.not_admin）：查看 key 是管理端点，query key 调不了。";
        err.hint = "在页面顶部第一个输入框填管理口令（admin token）后重试；⛔ 页面不会清掉你已经填的口令。";
      } else if (r.status === 401 || r.status === 403) {
        // 管理端点：照旧——清口令 + 回登录态 + 显示原因（安全语义不变）。
        logout("口令无效或权限不够（HTTP " + r.status + " " + err.code + "）：" + err.message);
      }
      throw err;
    });
  }).catch(function (e) {
    if (e && e.wbError) throw e;
    throw netError(e);
  });
}
function json(path, opts) {
  return request(path, opts).then(function (r) {
    try { return r.text ? JSON.parse(r.text) : null; } catch (e) {
      throw wbError(r.status, "BAD_JSON", "服务端返回的不是 JSON：" + r.text.slice(0, 200), "看服务端日志；这通常不是页面能修的问题。");
    }
  });
}
function post(path, body) {
  return json(path, { method: "POST", json: true, body: JSON.stringify(body) });
}
// 文本端点（REQ-161 追加修复）：`POST /api/r2rml` 走 `respond_text`，回的是 **text/turtle**，
// 不是 JSON。⛔ 别用 post()：它会 JSON.parse 一份 Turtle ⇒ 必抛 BAD_JSON，还把
// 「服务端返回了 Turtle」说成「去看服务端日志」。正文由 request() 的 {text} 直接给。
function postText(path, body) {
  return request(path, { method: "POST", json: true, body: JSON.stringify(body) })
    .then(function (r) { return r.text; });
}

// ══════════════════════════════════════════════════════════
// 错误 / 回执渲染（code / message / hint 三件都显示）
// ══════════════════════════════════════════════════════════
function errBox(e) {
  var box = el("div", "err");
  box.appendChild(el("div", "code", "[" + (e.code || "ERROR") + "]"));
  box.appendChild(el("div", "msg", e.message || String(e)));
  if (e.hint) box.appendChild(el("div", "hint", "怎么办：" + e.hint));
  return box;
}
function okBox(lines) {
  var box = el("div", "okbox");
  (lines || []).forEach(function (s) { box.appendChild(el("div", null, s)); });
  return box;
}
function showErr(node, e) { clear(node); node.appendChild(errBox(e)); }

// ══════════════════════════════════════════════════════════
// 登录态：sessionStorage（⛔ 不进地址栏、不写长期存储）
// ══════════════════════════════════════════════════════════
function remember() {
  try {
    sessionStorage.setItem(ADMIN_KEY, S.at);
  } catch (e) { /* 隐私模式下可能不可用：内存里的口令仍然有效 */ }
}
function forget() {
  try { sessionStorage.removeItem(ADMIN_KEY); } catch (e) {}
}
function logout(reason) {
  S.at = ""; S.connected = false;
  forget();
  $("login").hidden = false;
  $("token").value = "";
  txt($("who"), "未连接");
  setMsg(reason || "已退回登录态。", "bad");
}
function enter(fromStore) {
  var at = fromStore ? S.at : $("token").value.trim();
  if (!at) { setMsg("先填管理口令（admin token）。", "bad"); return; }
  S.at = at; S.connected = true;
  if (!fromStore) remember();
  $("login").hidden = true;
  txt($("who"), "已连接 · 口令只在本标签页");
  setMsg("已载入口令。", "ok");
  loadProjects().then(function () {
    if (!$("c-text").value) { /* 声明按需载入 */ }
  }).catch(function () {});
}
function restore() {
  try {
    S.at = sessionStorage.getItem(ADMIN_KEY) || "";
  } catch (e) { S.at = ""; }
  if (S.at) { $("token").value = S.at; enter(true); }
  else setMsg("未连接：填管理口令后点「载入」。浏览器导航带不了 Authorization 头，所以页面壳先给你登录框。", "");
}

// ══════════════════════════════════════════════════════════
// 二次确认：确认框写明**目标 + 后果**；取消 ⇒ 不发请求
// ══════════════════════════════════════════════════════════
function askConfirm(spec) {
  return new Promise(function (resolve) {
    txt($("m-title"), spec.title || "确认");
    var dl = $("m-rows");
    clear(dl);
    (spec.rows || []).forEach(function (r) {
      dl.appendChild(el("dt", null, r[0]));
      dl.appendChild(el("dd", null, r[1]));
    });
    var ok = $("m-ok");
    txt(ok, spec.ok || "确认执行");
    var done = false;
    function close(v) {
      if (done) return;
      done = true;
      $("modal").hidden = true;
      ok.onclick = null;
      $("m-cancel").onclick = null;
      resolve(v);
    }
    ok.onclick = function () { close(true); };
    $("m-cancel").onclick = function () { close(false); };
    $("modal").hidden = false;
  });
}
var CONFIRM = askConfirm;
function setConfirm(fn) { CONFIRM = fn; }
// 所有危险动作都从这里走：`run` 只在确认框返回 true 之后才执行。
function guarded(spec, run) {
  return CONFIRM(spec).then(function (ok) {
    if (!ok) return null;
    return run();
  });
}

// ══════════════════════════════════════════════════════════
// 通用表格
// ══════════════════════════════════════════════════════════
function cellNode(c) {
  var td = el("td");
  if (c && c.nodeType === 1) td.appendChild(c);
  else if (c && typeof c === "object" && c.text !== undefined) {
    txt(td, c.text);
    if (c.cls) td.className = c.cls;
  } else txt(td, (c === null || c === undefined) ? "—" : c);
  return td;
}
function makeTable(head, rows, emptyText) {
  if (!rows || !rows.length) return el("div", "empty", emptyText || "没有数据。");
  var t = el("table");
  var thead = el("thead"), tr = el("tr");
  (head || []).forEach(function (h) { tr.appendChild(el("th", null, h)); });
  thead.appendChild(tr);
  t.appendChild(thead);
  var tb = el("tbody");
  rows.forEach(function (row) {
    var r = el("tr");
    row.forEach(function (c) { r.appendChild(cellNode(c)); });
    tb.appendChild(r);
  });
  t.appendChild(tb);
  return t;
}
function kvList(pairs) {
  var dl = el("dl", "kv");
  pairs.forEach(function (p) {
    if (p[1] === undefined) return;
    dl.appendChild(el("dt", null, p[0]));
    dl.appendChild(cellNodeDd(p[1]));
  });
  return dl;
}
function cellNodeDd(v) {
  var dd = el("dd");
  if (v && v.nodeType === 1) dd.appendChild(v); else txt(dd, v);
  return dd;
}

// ══════════════════════════════════════════════════════════
// 1 · 项目与版本
// ══════════════════════════════════════════════════════════
function loadProjects() {
  return json("api/projects").then(function (d) {
    S.projects = d;
    renderProjects();
    fillVersionSelects();
    return d;
  }).catch(function (e) {
    showErr($("p-table"), e);
    txt($("p-summary"), "载入失败。");
    throw e;
  });
}
function renderProjects() {
  var d = S.projects;
  var sum = $("p-summary");
  clear(sum);
  if (!d) { txt(sum, "尚未载入。"); return; }
  var latest = d.versions && d.versions.length ? d.versions[d.versions.length - 1] : null;
  // 项目名是标识符（进 `:p` 路径），一律等宽 `.key`：靠视觉模式识别，比例字体读起来慢。
  sum.appendChild(el("b", "key", d.project));
  sum.appendChild(el("span", null, " · 共 " + ((d.versions || []).length) + " 版" +
    (latest ? (" · 最新第 " + latest.version + " 版" + (latest.frozen ? "（已冻结）" : "（未冻结）")) : " · 还没有任何版本")));
  var rows = (d.versions || []).map(function (v) {
    var ops = el("span");
    if (!v.frozen) {
      var b = el("button", "danger", "冻结…");
      b.onclick = function () { freezeVersion(d.project, v.version); };
      ops.appendChild(b);
    } else ops.appendChild(badge("已冻结", "ok"));
    return [
      v.version,
      v.frozen ? badge("已冻结", "ok") : badge("未冻结", "warn"),
      v.frozen_at || "—",
      v.has_snapshot ? "有" : "无",
      (v.objects === null || v.objects === undefined) ? "—" : v.objects,
      (v.relations === null || v.relations === undefined) ? "—" : v.relations,
      v.extracted_by ? (JSON.stringify(v.extracted_by).slice(0, 80) + "…（截断显示）") : "—",
      ops
    ];
  });
  var box = $("p-table");
  clear(box);
  box.appendChild(makeTable(["版本", "冻结", "冻结时间", "快照", "对象", "关系", "抽取来源", "操作"], rows, "还没有版本（先 import + apply）。"));
}
function freezeVersion(project, version) {
  guarded({
    title: "冻结第 " + version + " 版",
    rows: [
      ["目标", "项目 " + project + " · 第 " + version + " 版"],
      ["后果", "冻结后这一版**不能再改**：不能 apply、不能签条目；查询服务只服务已冻结的版本。冻结不可撤销。"]
    ],
    ok: "冻结第 " + version + " 版"
  }, function () {
    return post("api/freeze", { project: project, version: version }).then(function (r) {
      setMsg("第 " + version + " 版已冻结。", "ok");
      return loadProjects().then(function () { return r; });
    });
  }).then(function (r) {
    if (r === null) setMsg("已取消：没有发出任何请求。", "");
    else setMsg("冻结成功：" + JSON.stringify(r).slice(0, 200), "ok");
  }).catch(function (e) { showErr($("p-table"), e); });
}
function applyProposal() {
  var project = S.projects ? S.projects.project : "";
  var alias = $("p-alias").value.trim();
  if (!project) { setMsg("先载入项目。", "bad"); return; }
  if (!alias) { setMsg("写一个提案别名（p1 / UUID）。", "bad"); return; }
  guarded({
    title: "应用提案 " + alias,
    rows: [
      ["目标", "项目 " + project + " · 提案 " + alias],
      ["后果", "apply 会把该提案的条目写成**新版本**（对象 / 关系落库），不可撤销；已应用的提案再 apply ⇒ 409。"]
    ],
    ok: "应用 " + alias
  }, function () {
    return post("api/apply", { project: project, proposal: alias });
  }).then(function (r) {
    if (r === null) { setMsg("已取消：没有发出任何请求。", ""); return; }
    var out = $("p-apply-out");
    clear(out);
    out.appendChild(okBox(["已应用提案 " + alias + "。回执：" + JSON.stringify(r).slice(0, 400)]));
    loadProjects().catch(function () {});
  }).catch(function (e) { showErr($("p-apply-out"), e); });
}

// ══════════════════════════════════════════════════════════
// 3 · 评审
// ══════════════════════════════════════════════════════════
var SUGG = { accept: "接受", lean_accept: "倾向接受", review: "人工核对", reject: "拒绝", conflict: "冲突", none: "无建议" };
function suggOf(it) { return (it && it.suggestion) ? it.suggestion : { kind: "none", reason: "" }; }
function loadReview() {
  var p = $("r-proposal").value.trim() || "p1";
  S.reviewProposal = p; S.reviewPage = 1;
  return json("api/review?proposal=" + encodeURIComponent(p)).then(function (d) {
    S.review = d;
    renderReview();
    return d;
  }).catch(function (e) {
    S.review = null;
    showErr($("r-list"), e);
    txt($("r-summary"), "载入失败。");
    throw e;
  });
}
function renderReview() {
  var d = S.review, sum = $("r-summary");
  clear(sum);
  if (!d) { txt(sum, "尚未载入。"); return; }
  var p = d.progress || {};
  sum.appendChild(el("b", null, d.project));
  sum.appendChild(el("span", null, " · 提案 " + d.proposal + " · 基于 v" + d.base_version +
    " · 条目 " + ((d.items || []).length) +
    " · 已签 " + (p.signed || 0) + "/" + (p.total || 0) +
    " · 两轨通过 " + (p.passed || 0) + " · 被否 " + (p.vetoed || 0)));
  var list = $("r-list");
  clear(list);
  var items = d.items || [];
  var pages = Math.max(1, Math.ceil(items.length / PAGE_SIZE_REVIEW));
  if (S.reviewPage > pages) S.reviewPage = pages;
  var from = (S.reviewPage - 1) * PAGE_SIZE_REVIEW;
  var slice = items.slice(from, from + PAGE_SIZE_REVIEW);
  if (!slice.length) list.appendChild(el("div", "empty", "没有条目。"));
  slice.forEach(function (it) { list.appendChild(reviewCard(it, d)); });
  var pager = $("r-pager");
  clear(pager);
  pager.appendChild(el("button", null, "上一页")).onclick = function () { if (S.reviewPage > 1) { S.reviewPage--; renderReview(); } };
  pager.appendChild(el("span", null, "第 " + S.reviewPage + " / " + pages + " 页 · 共 " + items.length + " 条（每页 " + PAGE_SIZE_REVIEW + " 条）"));
  pager.appendChild(el("button", null, "下一页")).onclick = function () { if (S.reviewPage < pages) { S.reviewPage++; renderReview(); } };
  if (pages > 1) pager.appendChild(el("span", "note", "大列表只翻页显示，⛔ 不一次渲染全部。"));
}
function reviewCard(it, d) {
  var sg = suggOf(it);
  var card = el("article", "item" + ((it.biz === "rejected" || it.model === "rejected") ? " vetoed" : ""));
  var hd = el("div", "hd");
  hd.appendChild(el("span", "idx", "#" + it.item));
  hd.appendChild(el("span", "target key", it.target));
  hd.appendChild(el("span", "kind", it.kind));
  hd.appendChild(badge("建议：" + (SUGG[sg.kind] || sg.kind), sg.kind === "conflict" ? "bad" : (sg.kind === "reject" ? "bad" : (sg.kind === "accept" ? "ok" : "warn"))));
  if (it.confidence && typeof it.confidence.score === "number") {
    hd.appendChild(badge("置信度 " + it.confidence.score.toFixed(2) + (it.confidence.band ? " · " + it.confidence.band : ""), "acc"));
  }
  card.appendChild(hd);
  card.appendChild(el("div", "why", "建议依据：" + (sg.reason || "（服务端没给依据）")));
  if (it.reason) card.appendChild(el("div", "why", "阻断原因：" + it.reason));
  if (it.confidence && it.confidence.ai && it.confidence.ai.rationale) {
    card.appendChild(el("div", "why", "AI 理由：" + it.confidence.ai.rationale));
  }
  var tracks = el("div", "tracks");
  ["biz", "model"].forEach(function (track) {
    var wrap = el("div", "track");
    wrap.appendChild(el("span", "name", track === "biz" ? "业务轨：" : "建模轨："));
    var v = track === "biz" ? it.biz : it.model;
    wrap.appendChild(el("span", "st " + v, v === "accepted" ? "已接受" : (v === "rejected" ? "已拒绝" : "未签")));
    var a = el("button", "ok", "接受");
    var r = el("button", "bad", "拒绝");
    a.onclick = function () { decideItem(d, it, track, "accept"); };
    r.onclick = function () { decideItem(d, it, track, "reject"); };
    wrap.appendChild(a); wrap.appendChild(r);
    tracks.appendChild(wrap);
  });
  card.appendChild(tracks);
  if (it.evidence && it.evidence.length) {
    var lines = it.evidence.map(function (e) {
      if (typeof e === "string") return e;
      return [e.claim, e.table ? ("表 " + e.table) : "", e.column ? ("列 " + e.column) : "", e.inference ? ("推断 " + e.inference) : ""].filter(Boolean).join(" · ");
    }).join("；");
    card.appendChild(el("div", "why", "证据：" + lines));
  }
  var det = el("details");
  det.appendChild(el("summary", null, "候选定义（建模内容）"));
  det.appendChild(el("pre", null, JSON.stringify(it.payload, null, 2)));
  card.appendChild(det);
  return card;
}
function decideItem(d, it, track, decision) {
  post("api/decide", { project: d.project, proposal: d.proposal, item: it.item, track: track, decision: decision })
    .then(function (r) {
      if (r) {
        it.biz = r.biz; it.model = r.model; it.state = r.state;
        it.decided_at = r.decided_at; it.via = r.via;
        if (r.progress) d.progress = r.progress;
      }
      setMsg("#" + it.item + " 的" + (track === "biz" ? "业务轨" : "建模轨") + " → " + (decision === "accept" ? "已接受" : "已拒绝"), "ok");
      renderReview();
    })
    .catch(function (e) {
      setMsg("#" + it.item + " 签不下去：" + e.message + (e.hint ? ("（" + e.hint + "）") : ""), "bad");
    });
}

// ══════════════════════════════════════════════════════════
// 4 · 导出
// ══════════════════════════════════════════════════════════
function fillVersionSelects() {
  var d = S.projects;
  if (!d) return;
  ["e-version", "e2-version"].forEach(function (id) {
    var sel = $(id);
    clear(sel);
    (d.versions || []).forEach(function (v) {
      var o = document.createElement("option");
      o.value = String(v.version);
      o.textContent = "第 " + v.version + " 版" + (v.frozen ? "（已冻结）" : "（未冻结）");
      sel.appendChild(o);
    });
  });
}
function currentVersion(id) {
  var sel = $(id);
  if (!sel || !sel.value) return null;
  return parseInt(sel.value, 10);
}
function sha256Of(text) {
  if (!(window.crypto && window.crypto.subtle && window.TextEncoder)) return Promise.resolve(null);
  try {
    return window.crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)).then(function (buf) {
      var b = new Uint8Array(buf), out = "";
      for (var i = 0; i < b.length; i++) out += ("0" + b[i].toString(16)).slice(-2);
      return out;
    }).catch(function () { return null; });
  } catch (e) { return Promise.resolve(null); }
}
function downloadText(name, text) {
  var blob = new Blob([text], { type: "text/turtle" });
  var url = URL.createObjectURL(blob);
  var a = document.createElement("a");
  a.href = url; a.download = name;
  document.body.appendChild(a); a.click();
  document.body.removeChild(a);
  setTimeout(function () { URL.revokeObjectURL(url); }, 2000);
}
function exportTtl() {
  var project = S.projects ? S.projects.project : "";
  var version = currentVersion("e-version");
  var out = $("e-ttl-out");
  if (!project || !version) { setMsg("先载入项目并选版本。", "bad"); return; }
  var q = "api/export/ttl?project=" + encodeURIComponent(project) + "&version=" + version;
  if ($("e-model").checked) q += "&model_view=1";
  var schema = $("e-schema").value.trim();
  if (schema) q += "&schema=" + encodeURIComponent(schema);
  var tenancy = $("e-tenancy").value.trim();
  if (tenancy) q += "&tenancy=" + encodeURIComponent(tenancy);
  request(q).then(function (r) {
    clear(out);
    var name = project + "-v" + version + ($("e-model").checked ? "-model" : "") + ".ttl";
    out.appendChild(okBox(["TTL 已生成：" + fmtBytes(r.text.length) + " · 类型 " + (r.ctype || "—")]));
    return sha256Of(r.text).then(function (sha) {
      if (sha) out.appendChild(el("div", "note", "sha256（页面本地计算，可与 CLI 产物对拍）：" + sha));
      var b = el("button", "ok", "下载 " + name);
      b.onclick = function () { downloadText(name, r.text); };
      out.appendChild(b);
      var pre = el("pre");
      txt(pre, r.text.length > 2000 ? (r.text.slice(0, 2000) + "\n…（只预览前 2000 字符）") : r.text);
      out.appendChild(pre);
    });
  }).catch(function (e) { showErr(out, e); });
}
function exportR2rml() {
  var project = S.projects ? S.projects.project : "";
  var version = currentVersion("e2-version");
  var schema = $("e2-schema").value.trim();
  var out = $("e2-out");
  if (!project || !version) { setMsg("先载入项目并选版本。", "bad"); return; }
  if (!schema) { setMsg("R2RML 必须给 schema 的服务端路径（服务端按它判表 / 列）。", "bad"); return; }
  var body = { project: project, version: version, schema: schema, include_inherit: $("e2-inherit").checked };
  var ex = $("e2-exclude").value.trim();
  if (ex) body.exclude = ex;
  postText("api/r2rml", body).then(function (r) {
    clear(out);
    // 服务端回的是 text/turtle 原文（与 `ontok r2rml generate` 逐字节相同）：⛔ 页面不加工。
    var text = typeof r === "string" ? r : JSON.stringify(r);
    var name = project + "-v" + version + ".r2rml.ttl";
    out.appendChild(okBox(["R2RML 已生成：" + fmtBytes(text.length)]));
    var b = el("button", "ok", "下载 " + name);
    b.onclick = function () { downloadText(name, text); };
    out.appendChild(b);
    var pre = el("pre");
    txt(pre, text.length > 2000 ? (text.slice(0, 2000) + "\n…（只预览前 2000 字符）") : text);
    out.appendChild(pre);
  }).catch(function (e) { showErr(out, e); });
}

// 文本响应（TTL / R2RML / 声明原文）：`request` 返回 {text}，这里统一取正文。
function requestText(path, opts) {
  return request(path, opts).then(function (r) { return r.text; });
}

// ══════════════════════════════════════════════════════════
// 5 · 声明编辑
// ══════════════════════════════════════════════════════════
function loadConfig() {
  var kind = $("c-kind").value;
  S.declKind = kind;
  return request("api/config/" + encodeURIComponent(kind)).then(function (r) {
    $("c-text").value = r.text;
    txt($("c-meta"), "已载入 " + kind + "（" + fmtBytes(r.text.length) + "）。保存是整份替换；路径见服务端启动横幅。");
    clear($("c-out"));
    return r.text;
  }).catch(function (e) {
    txt($("c-meta"), "载入失败。");
    showErr($("c-out"), e);
    throw e;
  });
}
function saveConfig() {
  var kind = $("c-kind").value;
  var text = $("c-text").value;
  if (!text.trim()) { setMsg("编辑器是空的：PUT 的 body 是整份声明原文。", "bad"); return; }
  guarded({
    title: "保存声明 " + kind,
    rows: [
      ["目标", "声明文件 " + kind + "（服务端路径；整份替换）"],
      ["后果", "整份覆盖；服务端会先按 schema 校验表 / 列，校验失败 ⇒ 400 且原文件一个字节都不变。保存成功不可撤销（除非你手上还有旧内容）。"]
    ],
    ok: "保存 " + kind
  }, function () {
    // ⚠️ 失败时**不动**编辑器：不清空、不回滚、不重新载入（AC-007）。
    return request("api/config/" + encodeURIComponent(kind), {
      method: "PUT", body: text, json: true
    });
  }).then(function (r) {
    if (r === null) { setMsg("已取消：没有发出任何请求。", ""); return; }
    var body = null;
    try { body = JSON.parse(r.text); } catch (e) { body = null; }
    var out = $("c-out");
    clear(out);
    if (!body) { out.appendChild(okBox(["已保存（服务端回执不是 JSON）：" + r.text.slice(0, 200)])); return; }
    out.appendChild(okBox([
      "已保存 " + body.kind + "：" + fmtBytes(body.bytes) + " · sha256 " + shortSha(body.sha256),
      "上一版 sha256：" + (body.previous_sha256 || "（原来没有这个文件）"),
      "路径：" + body.path
    ]));
    setMsg("声明 " + kind + " 已保存。", "ok");
  }).catch(function (e) {
    // AC-007：显示 code / message / hint，**保留用户输入**。
    showErr($("c-out"), e);
    setMsg("保存失败：" + e.message + "（编辑器内容已保留，可直接改了再存）", "bad");
  });
}

// ══════════════════════════════════════════════════════════
// 6 · 审计
// ══════════════════════════════════════════════════════════
function loadAudit() {
  var limit = Math.max(1, Math.min(1000, parseInt($("a-limit").value, 10) || 100));
  $("a-limit").value = String(limit);
  return json("api/audit?limit=" + limit).then(function (d) {
    S.audit = d;
    renderAudit();
    return d;
  }).catch(function (e) {
    S.audit = null;
    showErr($("a-table"), e);
    txt($("a-summary"), "载入失败。");
    throw e;
  });
}
function renderAudit() {
  var d = S.audit, sum = $("a-summary");
  clear(sum);
  if (!d) { txt(sum, "尚未载入。"); return; }
  var want = $("a-action").value;
  var rows = (d.entries || []).filter(function (e) { return !want || e.action === want; });
  sum.appendChild(el("b", null, "共 " + (d.entries || []).length + " 条"));
  sum.appendChild(el("span", null, " · 顺序 " + (d.order === "oldest_first" ? "旧 → 新" : d.order) +
    (want ? (" · 只看 action=" + want + "：" + rows.length + " 条") : "") +
    " · 页面最多渲染 1000 行"));
  var box = $("a-table");
  clear(box);
  var view = rows.slice(-1000);
  box.appendChild(makeTable(["时间", "token（摘要）", "动作", "目标", "结果", "耗时(ms)"], view.map(function (e) {
    return [
      { text: e.ts || "—", cls: "mono" },
      { text: e.token || "—", cls: "key" },
      { text: e.action || "—", cls: "key" },
      e.target || "—", e.result || "—", e.elapsed_ms
    ];
  }), "还没有审计行（写操作与鉴权失败才会记）。"));
}

// ══════════════════════════════════════════════════════════
// 2 · 构建包（REQ-159）—— 建项目 / 上传 / 目标库 / 入库 / 待确认门禁 / 生效
// ══════════════════════════════════════════════════════════
// ⚠️ 查询台 / 目录浏览 的整段 JS（SPARQL 编辑器、四段元信息、分页目录、条目详情面板）
// 已于 2026-10-27 随视图一起移除（REQ-161b）：它们打的是 query serve 的三个读端点，
// 不属于「本机构建工作台」的范围。要查数据请用 Studio（/studio）或直接 curl query serve。
// 段号 = 左导航序号（用户看到的顺序），⛔ 不是文件里的先后：构建包这一段**留在文件末尾**，
// 好让「这个 tab 不许自己发裁决请求」能在这个标记之后一刀切
// （判据：the_bundles_view_declares_its_contract —— 它从本行往下扫，扫到裁决端点即红）。
// 这一组的端点在服务端是**并行开发**的：本 tab 的每次请求都带 notReady，
// 404 ⇒ 「服务端未就绪」并且**不登出**（见上面的 request 层）。
// 激活成功后的**固定警示**（契约原文，⛔ 一个字都别改）：
// 建了新项目、叠了构建包也不代表对外能查 —— `query serve` 目前仍绑单一项目。
var ACTIVATE_NOTE = "已入库并激活。对外服务需等 P1b（一套部署多套本体）上线后生效 —— " +
  "当前 query serve 仍绑定单一项目。";
// key 明文**按需拉取**（GET …/key），只放在内存里供复制：
// ⛔ 不进 sessionStorage、不写长期存储、不回填任何输入框（能再次查看靠的是「再拉一次」，不是缓存）。
var LAST_KEY = null;

function bundleProject() { return (S.projects && S.projects.project) ? S.projects.project : ""; }
function bundlePath(suffix) {
  var p = bundleProject();
  return p ? ("api/projects/" + encodeURIComponent(p) + suffix) : "";
}
// 本 tab 的统一发包口：一律带 notReady（404 ⇒ 服务端未就绪）。
function bjson(path, method, body) {
  var opts = { method: method || "GET", notReady: true };
  if (body !== undefined) { opts.json = true; opts.body = JSON.stringify(body); }
  return json(path, opts);
}
// 组件清单的「计数」字段名没有统一口径（服务端按组件给），这里只认几个常见键，
// 一个都没有就显示 —（比猜一个数字更诚实）。
function countOf(c) {
  if (!c) return "—";
  if (typeof c.count === "number") return c.count;
  if (typeof c.item_count === "number") return c.item_count;
  if (typeof c.total === "number") return c.total;
  if (c.items && c.items.length !== undefined) return c.items.length;
  if (c.entries && c.entries.length !== undefined) return c.entries.length;
  return "—";
}
function flagOf(v) { return (v === true || v === "true") ? "是" : "否"; }

function loadBundles() {
  txt($("b-project"), "载入中…");
  return json("api/projects").then(function (d) {
    // ⚠️ 本页**新建**的项目不会被 GET /api/projects 报出来（它只报服务端启动时的那个），
    // 所以有 override 时不能拿服务端的回执把当前项目冲掉。
    if (!S.projectOverride) S.projects = d;
    txt($("b-project"), bundleProject() || "—");
    clear($("b-msg"));
    if (S.projectOverride) {
      $("b-msg").appendChild(el("div", "note",
        "当前项目已切到本页新建的「" + S.projectOverride + "」：GET /api/projects 只报服务端启动时配置的那个项目，别被它误导。"));
    }
    loadChecklist().catch(function () {});
    return d;
  }).catch(function (e) {
    txt($("b-project"), "—");
    showErr($("b-msg"), e);
    throw e;
  });
}

// 新建项目：POST /api/projects（建项目 + 发 key）。端点是**路径作用域** `:p`，不读 svc.project，
// 所以建出来的项目与 console 启动参数无关，可以立刻接着上传 / 解析 / 激活。
// 前端先校验，不合格**就地报错、不发请求**（省一次 400）。`key_label` 必填（审计归属）。
// 项目名的唯一校验点，口径与 `manifest.project` 的 pattern 完全一致：
// `^[a-z0-9][a-z0-9_-]{1,62}$` —— 小写字母 / 数字 / 连字符 / **下划线**，
// 首字符必须是字母或数字，2–63 位。大写 / 空格 / 斜杠 / 点 / `..` 仍然全禁。
// ⚠️ 改这条正则必须**同时**改下面 BAD_PROJECT_NAME 的文案，否则用户会看到「说得比做的严」。
function projectNameOk(name) {
  return /^[a-z0-9][a-z0-9_-]{1,62}$/.test(name);
}
function createProject() {
  var name = $("b-new-project").value.trim();
  var label = $("b-new-label").value.trim();
  var keyLabel = $("b-new-key-label").value.trim();
  var out = $("b-new-out");
  clear(out);
  if (!projectNameOk(name)) {
    out.appendChild(errBox(wbError(400, "BAD_PROJECT_NAME",
      "项目名不合法：" + (name || "（空）"),
      "只许小写字母 / 数字 / 连字符 / 下划线，首字符是字母或数字，长度 2–63（例如两个词用连字符或下划线连起来；大写、空格、斜杠、点都不行）。")));
    setMsg("项目名不合法：没有发出任何请求。", "bad");
    return;
  }
  if (!keyLabel) {
    out.appendChild(errBox(wbError(400, "KEY_LABEL_REQUIRED",
      "key 用途（key_label）必填：服务端用它记审计归属。",
      "填一句「这把 key 谁在用」（系统名 / 角色名）；⛔ 不要把密钥本身填进来。")));
    setMsg("key 用途必填：没有发出任何请求。", "bad");
    return;
  }
  out.appendChild(el("div", "note", "建立项目中…"));
  return bjson("api/projects", "POST", { project: name, label: label, key_label: keyLabel }).then(function (r) {
    r = r || {};
    var created = r.project || name;
    clear(out);
    // 当前项目切到新项目：后面的上传 / DSN / 解析 / 激活 / 查看 key 都按它拼路径。
    S.projectOverride = created;
    S.projects = { project: created, versions: [] };
    txt($("b-project"), created);
    clear($("b-msg"));
    showKey(r);   // 回执里带 key：直接渲染（label / created_at / 已吊销 一并显示）
    out.appendChild(okBox([
      "已建立项目 " + created + "（key_kind " + (r.key_kind || "—") + " · key_label " + (r.key_label || "—") + "）",
      "key_digest " + (r.key_digest || "—") + "（摘要可留档；明文在上面 key 区块，可随时「查看 key」再取）",
      "下一步：在上面「上传并校验」选构建包 —— 解析 / 激活都按项目 " + created + " 走。"
    ]));
    setMsg("已建立项目 " + created + "：可以上传构建包了。", "ok");
    return r;
  }).catch(function (e) {
    clear(out);
    if (e && e.status === 409) {
      out.appendChild(errBox(wbError(409, "PROJECT_EXISTS",
        "项目已存在：" + e.message,
        "换一个项目名；如果这就是你想用的那个项目，直接在上面「上传并校验」里传构建包即可（不用再建）。")));
      setMsg("项目已存在：直接上传构建包即可。", "bad");
      return;
    }
    showErr(out, e);   // 404 ⇒ request 已改成「服务端未就绪」
    setMsg("建立项目失败：" + e.message, "bad");
  });
}

// 上传：⛔ 不能走 post()（它 JSON.stringify 后 body 变成字符串，文件字节当场坏掉）。
// 走 request 层唯一的那处 fetch，body 直接给文件句柄 + octet-stream。
function uploadBundle() {
  var p = bundleProject();
  var input = $("b-file");
  var f = (input.files && input.files[0]) ? input.files[0] : null;
  var state = $("b-state");
  if (!p) { setMsg("先载入项目（点「刷新」）。", "bad"); return; }
  if (!f) { setMsg("先选一个 .ontokbundle 文件。", "bad"); return; }
  clear($("b-components"));
  clear(state);
  txt(state, "上传中…（" + fmtBytes(f.size) + "，原始字节，不套 JSON）");
  return json(bundlePath("/bundles"), {
    method: "POST", body: f, ctype: "application/octet-stream", notReady: true
  }).then(function (res) {
    showBundle(res || {});
    setMsg("已上传：" + ((res && res.bundle_id) || "（回执里没有 bundle_id）"), "ok");
    return res;
  }).catch(function (e) {
    showErr(state, e);
    setMsg("上传失败：" + e.message, "bad");
  });
}

function showBundle(res) {
  S.bundle = res || null;
  var state = $("b-state"), box = $("b-components");
  clear(state);
  clear(box);
  if (!res) { txt(state, "还没有上传。"); renderBundleGates(); return; }
  state.appendChild(kvList([
    ["state", res.state ? badge(res.state, res.state === "active" ? "ok" : (res.state === "validated" ? "acc" : "warn")) : "—"],
    ["bundle_id", res.bundle_id || "—"],
    ["sha256_verified", res.sha256_verified === true ? badge("已校验", "ok")
      : (res.sha256_verified === false ? badge("校验未通过", "bad") : "—")],
    ["requires_db", flagOf(res.requires_db)]
  ]));
  var comps = res.components || {};
  box.appendChild(makeTable(["组件", "计数", "requires_dsn", "dialect", "状态"], ["ontology", "am", "db"].map(function (k) {
    var c = comps[k] || null;
    return [k, c ? countSummary(k, c) : "—", dsnFlag(c), c ? (c.dialect || "—") : "—",
      compStatus(k)];
  }), "组件清单为空。"));
  needDsn(res);
  renderBundleGates();
}

// `requires_dsn` 列：**字段不存在就显示 —**（组件级契约里 ontology / am 没有这个字段）。
// ⛔ 别拿「否」顶上去 —— 那是替服务端下一个它没下的结论（与「计数掉成 —」同一类毛病）。
function dsnFlag(c) {
  if (!c || c.requires_dsn === undefined || c.requires_dsn === null) return "—";
  return flagOf(c.requires_dsn);
}

// 组件「计数」列：按组件给出人看得懂的摘要（字段名以契约 §五-2 为准）。
// 缺哪个字段就省略那一段（例如只有 entity_types 就显示「30 类」），一个都没有才退回 countOf ——
// ⛔ 别因为缺字段就把整列掉成「—」：FDE 会以为 manifest 里没写计数（服务端其实已经验过）。
function countSummary(kind, c) {
  if (!c) return "—";
  var parts = [];
  function add(v, unit) {
    if (typeof v === "number" && isFinite(v)) parts.push(v + " " + unit);
  }
  if (kind === "ontology") {
    add(c.entity_types, "类");
    add(c.link_types, "关系");
    add(c.properties, "属性");
  } else if (kind === "am") {
    add(c.entities, "实体");
    add(c.sources, "源");
  } else if (kind === "db") {
    // 契约没规定 db 的计数：有 tables / columns 才显示，⛔ 不编。
    add(c.tables, "表");
    add(c.columns, "列");
  }
  return parts.length ? parts.join(" / ") : countOf(c);
}

// 组件状态（照服务端 P1a 的真实行为写，⛔ 别夸大成「已经可用」）：
// `am/` 在 P1a 只被**校验**、不解析入 metadb ⇒ 固定写「AM 随包收录，P1b 生效」，用 warn 色。
// ontology / db 的状态不在这里下结论（本体由 import 回执的版本 / counts 说话）。
function compStatus(k) {
  if (k === "am") return badge("AM 随包收录，P1b 生效", "warn");
  return "—";
}

// requires_dsn ⇒ 自动展开 DSN 面板（不是让用户自己猜要不要填）。
function needDsn(res) {
  var db = (res && res.components) ? res.components.db : null;
  var need = !!(res && (res.requires_db === true ||
    (db && (db.requires_dsn === true || db.requires_dsn === "true"))));
  S.needDsn = need;
  $("b-dsn").hidden = !need;
  if (!need) S.dsnOk = true;   // 不需要库 ⇒ DSN 这道门禁不参与
  return need;
}

// DSN 由前端拼：user / password 必须 encodeURIComponent，host / port / db 原样。
function dsnValue() {
  var host = $("b-dsn-host").value.trim();
  var port = $("b-dsn-port").value.trim() || "5432";
  var db = $("b-dsn-db").value.trim();
  var user = $("b-dsn-user").value.trim();
  var pass = $("b-dsn-pass").value;
  var ssl = $("b-dsn-ssl").value || "prefer";
  if (!host || !db || !user) return "";
  return "postgresql://" + encodeURIComponent(user) + ":" + encodeURIComponent(pass) +
    "@" + host + ":" + port + "/" + db + "?sslmode=" + ssl;
}

function saveDsn() {
  var p = bundleProject();
  var out = $("b-dsn-out");
  if (!p) { setMsg("先载入项目（点「刷新」）。", "bad"); return; }
  var dsn = dsnValue();
  if (!dsn) { setMsg("host / 库 / user 都要填（DSN 由本页拼）。", "bad"); return; }
  clear(out);
  return bjson(bundlePath("/database"), "PUT", { dsn: dsn, dialect: "postgres" }).then(function (r) {
    r = r || {};
    clear(out);
    out.appendChild(okBox([
      "DSN 已保存：database_id " + (r.database_id || "—"),
      "host " + (r.host || "—") + " · database " + (r.database || "—") + " · user " + (r.user || "—"),
      "回执里没有 dsn（服务端本来就不回），本页也不显示；口令框已清空，不回填。"
    ]));
    $("b-dsn-pass").value = "";   // ⭐ 只清口令框
    S.dsnOk = false;              // 保存 ≠ 连通：还要「测试连通」过了才放行入库
    out.appendChild(el("div", "note", "下一步点「测试连通」：通过后「解析入库」才会亮。"));
    renderBundleGates();
    setMsg("DSN 已保存（还没验证连通）。", "ok");
    return r;
  }).catch(function (e) {
    showErr(out, e);
    setMsg("保存 DSN 失败：" + e.message, "bad");
  });
}

function testDsn() {
  var p = bundleProject();
  var out = $("b-dsn-out");
  if (!p) { setMsg("先载入项目（点「刷新」）。", "bad"); return; }
  clear(out);
  out.appendChild(el("div", "note", "测试中…（打的是服务端已保存的那份连接）"));
  return bjson(bundlePath("/database/test"), "POST", {}).then(function (r) {
    r = r || {};
    clear(out);
    if (r.ok === true) {
      out.appendChild(okBox([
        "连通成功 · 延迟 " + (r.latency_ms === undefined ? "—" : (r.latency_ms + " ms")),
        "server_version：" + (r.server_version || "—")
      ]));
      S.dsnOk = true;
      setMsg("目标库连通。", "ok");
    } else {
      // ⚠️ 连通失败也是 HTTP 200：回执里是 ok=false + error{code,message}，
      // 所以这里**自己**造一个三段式错误框，而不是等 request 抛（否则看起来像网络断了）。
      var err = r.error || {};
      S.dsnOk = false;
      out.appendChild(errBox(wbError(200, err.code || "DSN_TEST_FAILED",
        err.message || "连通测试失败（服务端回 ok=false，但没给原因）。",
        err.hint || "查库账号 / 网络 / sslmode；这一条 HTTP 是 200，不是断网。")));
      setMsg("目标库连不通：" + (err.message || "服务端没给原因"), "bad");
    }
    renderBundleGates();
    return r;
  }).catch(function (e) {
    S.dsnOk = false;
    showErr(out, e);
    renderBundleGates();
    setMsg("测试失败：" + e.message, "bad");
  });
}

function importBundle() {
  var p = bundleProject();
  var b = S.bundle || {};
  var out = $("b-import-out");
  if (!p || !b.bundle_id) { setMsg("先上传一个包，拿到 bundle_id。", "bad"); return; }
  if (S.needDsn && S.dsnOk !== true) { setMsg("这个包要目标库：先把 DSN 保存并让「测试连通」通过。", "bad"); return; }
  clear(out);
  out.appendChild(el("div", "note", "解析入库中…"));
  return bjson(bundlePath("/bundles/" + encodeURIComponent(b.bundle_id) + "/import"), "POST", {}).then(function (r) {
    r = r || {};
    clear(out);
    // ⚠️ 重复 import 是 **200 + already_imported:true**（不是 409）：那是**幂等成功**，
    // 不是错误。所以不能进 catch、不能清状态、不能禁用按钮 —— 版本照常算「已入库」。
    if (r.version !== undefined) b.version = r.version;
    if (r.state) b.state = r.state;
    S.bundle = b;
    if (r.already_imported === true) {
      out.appendChild(okBox([
        "这个包已经导入过（幂等）：版本 " + (r.version === undefined ? "—" : ("第 " + r.version + " 版")) +
          "，没有重复建版本",
        "state：" + (r.state || "—") + " · 后续步骤照常：这个版本仍算「已入库」，可以继续往下走。"
      ]));
    } else {
      out.appendChild(okBox([
        "版本：" + (r.version === undefined ? "—" : ("第 " + r.version + " 版")),
        "state：" + (r.state || "—")
      ]));
    }
    var counts = r.counts || {};
    out.appendChild(el("div", "note", "counts："));
    var keys = Object.keys(counts);
    if (keys.length) out.appendChild(kvList(keys.map(function (k) { return [k, counts[k]]; })));
    else out.appendChild(el("div", "note", "（服务端没给 counts）"));
    var warns = r.warnings || [];
    if (warns.length) {
      var ul = el("ul", "warns");
      warns.forEach(function (w) { ul.appendChild(el("li", null, w)); });
      out.appendChild(ul);
    } else out.appendChild(el("div", "note", "warnings：0 条。"));
    setMsg(r.already_imported === true
      ? "幂等：这个包之前就导入过，没有重复建版本。"
      : "已解析入库。", "ok");
    renderBundleGates();
    return r;
  }).catch(function (e) {
    showErr(out, e);
    setMsg("解析入库失败：" + e.message, "bad");
  });
}

// 待确认清单的口径**以服务端的聚合为准**（`GET /api/review` 的 `progress`，见 review_ui）：
//
// | 数 | 取哪 | 为什么不能自己数 |
// | --- | --- | --- |
// | 待确认 N | `progress.pending`（= total − signed） | `signed` 的语义是**两轨都定了**；自己按单字段数会把「只签了一轨」漏掉 |
// | 阻断 M | `progress.vetoed` | 某轨被否才是阻断；页面**没有** item 级的 `blocking` / `decision` 字段 |
//
// item 的真实字段只有：`item`（序号）/ `kind` / `target` / `biz` / `model` / `state` /
// `reason`（仅 state=="vetoed" 时有值）/ `confidence` / `suggestion{kind,reason}` / `evidence` /
// `decided_at{biz,model}` / `via{biz,model}` / `payload`。
//
// ⛔ `item.decision` 这个字段**不存在**，别再造一个；单条是否未决只看两轨。
function isPendingItem(it) {
  if (!it) return false;
  return it.biz === "pending" || it.model === "pending";
}
// 汇总：老服务端没有 `progress` 时退化成「按条目数」，并把这件事写在行里 ——
// ⛔ 不许静默当成 0（那会让门禁误开，且没人知道数是估的）。
function checklistSummary(d) {
  var items = (d && d.items) || [];
  var progress = (d && d.progress) ? d.progress : null;
  if (progress && typeof progress.pending === "number") {
    return {
      n: progress.pending,
      m: (typeof progress.vetoed === "number") ? progress.vetoed : 0,
      estimated: false
    };
  }
  return {
    n: items.filter(isPendingItem).length,
    m: items.filter(function (it) { return it && it.state === "vetoed"; }).length,
    estimated: true
  };
}
function loadChecklist() {
  var box = $("b-checklist");
  return json("api/review?proposal=p1", { notReady: true }).then(function (d) {
    S.checklist = d || null;
    renderChecklist();
    return d;
  }).catch(function (e) {
    S.checklist = null;
    S.pendingCount = null;
    clear(box);
    showErr(box, e);
    renderBundleGates();
    throw e;
  });
}
function renderChecklist() {
  var box = $("b-checklist");
  clear(box);
  var d = S.checklist;
  if (!d) { txt(box, "尚未载入。"); S.pendingCount = null; renderBundleGates(); return; }
  var items = d.items || [];
  var todo = items.filter(isPendingItem);
  var sum = checklistSummary(d);
  S.pendingCount = sum.n;   // 门禁只看这一个数（来源：progress.pending）
  box.appendChild(el("b", null, "待确认 " + sum.n + " 条（其中阻断 " + sum.m + " 条）"));
  box.appendChild(el("span", null, " · 提案 " + (d.proposal || "—") +
    " · 基于第 " + (d.base_version === undefined ? "—" : d.base_version) + " 版" +
    (d.frozen ? " · 已冻结" : "")));
  if (sum.estimated) {
    box.appendChild(el("div", "note",
      "服务端没给 progress，按条目估算（" + todo.length + " 条未决）—— 这是估算，不是服务端签过的数。"));
  }
  if (!sum.n) {
    box.appendChild(el("div", "note", "待确认 0 条：门禁已放开，可以「冻结并切流量」。"));
    renderBundleGates();
    return;
  }
  box.appendChild(el("div", "note", "本 tab 只汇总与门禁：逐条裁决在「评审」tab 做（点「去评审」）。"));
  var list = el("div");
  todo.forEach(function (it) {
    var sg = suggOf(it);
    var row = el("div", "hd");
    row.appendChild(el("span", "idx", "#" + (it.item === undefined ? "—" : it.item)));
    row.appendChild(el("span", "target", it.target || "—"));
    if (sg.kind) {
      row.appendChild(badge("建议：" + (SUGG[sg.kind] || sg.kind),
        sg.kind === "conflict" || sg.kind === "reject" ? "bad"
          : (sg.kind === "accept" || sg.kind === "lean_accept" ? "ok" : "warn")));
    }
    ["biz", "model"].forEach(function (track) {
      var v = it[track];
      row.appendChild(el("span", "st " + (v || "pending"),
        (track === "biz" ? "业务轨：" : "建模轨：") +
        (v === "accepted" ? "已接受" : (v === "rejected" ? "已拒绝" : "未签"))));
    });
    if (it.state === "vetoed") row.appendChild(badge("被否", "bad"));
    var go = el("button", null, "去评审");
    go.onclick = function () { gotoReview(d.proposal || "p1"); };
    row.appendChild(go);
    list.appendChild(row);
  });
  box.appendChild(list);
  renderBundleGates();
}
function gotoReview(proposal) {
  $("r-proposal").value = proposal || "p1";
  S.review = null;          // 让 switchTab 按新别名重新载入，而不是复用上一次的
  switchTab("review");
}

// 门禁：入库要看 bundle_id（+ 需要库时 DSN 连通）；生效还要「待确认 0 条」
// （`pendingCount` 只有一个来源：`progress.pending`，见 checklistSummary）。
function renderBundleGates() {
  var b = S.bundle || {};
  var imported = !!(b.version || b.state === "imported");
  var dsnOk = (!S.needDsn) || (S.dsnOk === true);
  $("b-import").disabled = !(b.bundle_id && dsnOk);
  $("b-activate").disabled = !(imported && S.pendingCount === 0);
}

function activateBundle() {
  var p = bundleProject();
  var b = S.bundle || {};
  if (!p || !b.bundle_id) { setMsg("先上传并解析入库。", "bad"); return; }
  if (S.pendingCount !== 0) {
    setMsg("门禁：待确认清单" + (S.pendingCount === null || S.pendingCount === undefined
      ? "还没载入" : ("还有 " + S.pendingCount + " 条（progress.pending）")) + "，不能激活。", "bad");
    return;
  }
  return guarded({
    title: "冻结并切流量：第 " + (b.version === undefined ? "—" : b.version) + " 版",
    rows: [
      ["目标", "项目 " + p + " · 构建包 " + b.bundle_id + (b.version === undefined ? "" : (" · 第 " + b.version + " 版"))],
      ["后果", "这一版会被冻结、查询服务随即切过去：之后不能再 apply、不能再签条目；切换不可撤销（回退要另走一次发布）。"]
    ],
    ok: "冻结并切流量"
  }, function () {
    return bjson(bundlePath("/bundles/" + encodeURIComponent(b.bundle_id) + "/activate"), "POST", {});
  }).then(function (r) {
    if (r === null) { setMsg("已取消：没有发出任何请求。", ""); return; }
    r = r || {};
    var out = $("b-import-out");
    out.appendChild(okBox([
      "已激活：state " + (r.state || "—") +
      " · 版本 " + (r.version === undefined ? "—" : r.version) +
      " · ontology_version " + (r.ontology_version === undefined ? "—" : r.ontology_version)
    ]));
    // ⚠️ 固定警示（照契约原文）：本页可以建新项目并激活，但**对外查询**还要等 P1b。
    // 少这一句，用户会拿着新 key 去查、拿到 503/404，然后来问「为什么建了不能用」。
    out.appendChild(el("div", "note", ACTIVATE_NOTE));
    b.state = r.state || "active";
    S.bundle = b;
    setMsg("已冻结并切流量。", "ok");
    renderBundleGates();
    // 激活成功后**自动拉一次** key（服务端会为这次查看写一条 reveal_key 审计），
    // 不再从内存/回执里打印；`quiet` 表示别覆盖上面那条「已激活」状态。
    // 拉失败只影响 key 区块（显示 code/message + 按钮还在），不影响激活结果。
    return viewKey(true);
  }).catch(function (e) {
    showErr($("b-import-out"), e);
    setMsg("激活失败：" + e.message, "bad");
  });
}

// key 是**按需拉取**的：明文只放内存变量 `LAST_KEY`（给复制用），⛔ 不写 sessionStorage、
// 不写长期存储、不回填输入框。能再次查看的代价是每次查看都要留痕 —— 服务端写 reveal_key 审计。
function showKey(row) {
  row = row || {};
  LAST_KEY = row.key || "";
  $("b-key").hidden = !LAST_KEY;
  txt($("b-key-val"), LAST_KEY || "（服务端这次没有返回 key）");
  var meta = $("b-key-meta");
  clear(meta);
  if (row.label) meta.appendChild(el("span", null, "label：" + row.label + " · "));
  if (row.created_at) meta.appendChild(el("span", null, "创建于 " + row.created_at + " · "));
  if (row.revoked_at) {
    meta.appendChild(badge("已吊销", "bad"));
    meta.appendChild(el("span", null, " 吊销于 " + row.revoked_at));
  }
  return LAST_KEY;
}
// 查看 key：GET …/key（每次调用服务端都记一条 reveal_key 审计）。
// `quiet`：激活后自动拉取时用 —— 别把「已激活」那条状态栏消息覆盖掉。
function viewKey(quiet) {
  var p = bundleProject();
  var out = $("b-key-out");
  if (!p) { setMsg("先载入项目（点「刷新」）。", "bad"); return; }
  clear(out);
  out.appendChild(el("div", "note", "读取中…（这一次查看会被记进审计）"));
  return json(bundlePath("/key"), { notReady: true, needAdmin: true }).then(function (r) {
    clear(out);
    showKey(r || {});
    if (!quiet) setMsg("已读取 key（服务端记了一条 reveal_key 审计）。", "ok");
    return r;
  }).catch(function (e) {
    // 404 ⇒ request 已改成「服务端未就绪」；403 ⇒ 「需要 admin token」。
    // 两种情况都**不动**内存里上一次拿到的值，也不假装拿到 key。
    showErr(out, e);
    setMsg((quiet ? "已冻结并切流量；但读 key 失败：" : "读不到 key：") + e.message, quiet ? "" : "bad");
  });
}
function copyKey() {
  if (!LAST_KEY) { setMsg("还没有可复制的 key（先点「查看 key」）。", "bad"); return; }
  if (navigator && navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(LAST_KEY).then(function () {
      setMsg("key 已复制到剪贴板（仍然只在内存里）。", "ok");
    }).catch(function () {
      setMsg("复制被浏览器拒绝：请手动选中 key 那一行复制。", "bad");
    });
  } else {
    setMsg("这个浏览器不给剪贴板权限：请手动选中 key 那一行复制。", "bad");
  }
}
function rotateKey() {
  var p = bundleProject();
  if (!p) { setMsg("先载入项目（点「刷新」）。", "bad"); return; }
  return guarded({
    title: "换 key（项目 " + p + "）",
    rows: [
      ["目标", "项目 " + p + " 的访问 key（key/rotate）"],
      ["后果", "旧 key **立刻失效**：还在用旧 key 的调用方会 401；新 key 可以随时用「查看 key」再看到（每次查看写一条 reveal_key 审计），但旧 key 换不回来。（P1b 起可用；当前服务端会返回未实现）"]
    ],
    ok: "换 key"
  }, function () {
    return bjson(bundlePath("/key/rotate"), "POST", {});
  }).then(function (r) {
    if (r === null) { setMsg("已取消：没有发出任何请求。", ""); return; }
    r = r || {};
    showKey(r);
    var out = $("b-import-out");
    if (r.key_digest) out.appendChild(el("div", "note", "key_digest：" + r.key_digest + "（摘要可留档；明文可以随时「查看 key」再取）"));
    setMsg("已换 key（新明文已显示，也可以随时「查看 key」）。", "ok");
  }).catch(function (e) {
    showErr($("b-import-out"), e);
    setMsg("换 key 失败：" + e.message, "bad");
  });
}

// ══════════════════════════════════════════════════════════
// tab 切换（无 URL 状态：地址栏永远不带口令）
// ══════════════════════════════════════════════════════════
var TABS = ["projects", "bundles", "review", "export", "config", "audit"];
function switchTab(name) {
  S.tab = name;
  TABS.forEach(function (t) {
    $("v-" + t).hidden = (t !== name);
    // 左导航：`is-on` 只是视觉态，⛔ 别整段重写 className（会把 nav-item 一起抹掉）。
    var btn = $("tab-" + t);
    if (btn.classList) btn.classList.toggle("is-on", t === name);
    else btn.className = (t === name) ? "nav-item is-on" : "nav-item";
  });
  var host = $("views");
  if (host) host.scrollTop = 0;
  if (name === "review" && !S.review) loadReview().catch(function () {});
  if (name === "audit" && !S.audit) loadAudit().catch(function () {});
  if (name === "export" && !S.projects) loadProjects().catch(function () {});
  if (name === "bundles") loadBundles().catch(function () {});
}

// ══════════════════════════════════════════════════════════
// 绑定 + 启动
// ══════════════════════════════════════════════════════════
$("go").onclick = function () { enter(false); };
$("token").addEventListener("keydown", function (e) { if (e.key === "Enter") enter(false); });
TABS.forEach(function (t) { $("tab-" + t).onclick = function () { switchTab(t); }; });
$("p-refresh").onclick = function () { loadProjects().catch(function () {}); };
$("p-apply").onclick = applyProposal;
$("r-load").onclick = function () { loadReview().catch(function () {}); };
$("r-proposal").value = "p1";
$("e-ttl").onclick = exportTtl;
$("e2-go").onclick = exportR2rml;
$("c-load").onclick = function () { loadConfig().catch(function () {}); };
$("c-save").onclick = saveConfig;
$("c-kind").onchange = function () { loadConfig().catch(function () {}); };
$("a-load").onclick = function () { loadAudit().catch(function () {}); };
$("a-action").onchange = function () { if (S.audit) renderAudit(); };
$("b-refresh").onclick = function () { loadBundles().catch(function () {}); };
$("b-new-submit").onclick = createProject;
$("b-upload").onclick = uploadBundle;
$("b-dsn-save").onclick = saveDsn;
$("b-dsn-test").onclick = testDsn;
$("b-import").onclick = importBundle;
$("b-checklist-refresh").onclick = function () { loadChecklist().catch(function () {}); };
$("b-activate").onclick = activateBundle;
$("b-rotate").onclick = rotateKey;
$("b-key-copy").onclick = copyKey;
$("b-key-view").onclick = function () { viewKey(); };

restore();

// ⭐ 纯函数 / 可注入点的唯一出口：`tests/fixtures/req125/page_probe.js` 从这里跑
// guarded（取消不发请求）、el / makeTable（XSS）、remember（sessionStorage）。
if (typeof window !== "undefined") {
  window.WBCORE = {
    guarded: guarded,
    setConfirm: setConfirm,
    el: el,
    makeTable: makeTable,
    remember: remember,
    request: request,
    state: S,
    tabs: TABS,
    actions: { apply: applyProposal, freeze: freezeVersion, saveConfig: saveConfig }
  };
}
</script>
</body>
</html>
"####;

/// `GET /` 的 CSP。
///
/// # `'unsafe-inline'` 为什么在这里、代价是什么、什么时候必须去掉（REQ-125 · L-2）
///
/// **为什么需要**：本页是**单文件内联页** —— HTML / CSS / JS 全部写死在
/// [`CONSOLE_HTML`] 里，一个外部资源都不引（AC-002 的「零外部依赖」）。
/// 内联 `<style>` / `<script>` 在 CSP 里属于 *inline* 资源，不放 `'unsafe-inline'`
/// 浏览器会**整段拒执行** ⇒ 页面只剩一张静态骨架，登录、八个视图、危险操作确认
/// 全部失效。也就是说：这两处 `'unsafe-inline'` 不是「图省事」，而是
/// 「单文件 + 零外链」这条纪律在 CSP 上的**必然代价**；拿掉它就必须先把脚本/样式
/// 拆成外部文件，而那等于改需求边界。
///
/// **代价（必须承认，不许写成「已防护」）**：`'unsafe-inline'` 让浏览器不再拦截
/// 内联脚本执行 ⇒ CSP 对「注入一段 `<script>` 或 `onerror=`」这条路的防护被削弱，
/// 页面自己必须兜住。本页的兜法是**只走 `textContent`**：所有服务端字符串
/// （对象名 / 说明 / 错误信息 / 回执）都经 `el()` / `txt()` 写入，页面里
/// `innerHTML` / `outerHTML` / `insertAdjacentHTML` / `document.write` / `eval(`
/// **各 0 处**（判据：`tests/test_req125_page.rs::ac010_no_innerhtml_anywhere`；
/// 交互层另由 `tests/fixtures/req125/page_probe.js` 用「赋值即抛」的假 DOM 钉住）。
/// ⛔ 谁把某处渲染改成 `innerHTML`，等于同时打掉两层的防护。
///
/// **将来什么时候必须去掉**：一旦脚本或样式被拆成外部 `.js` / `.css`
/// （例如为了复用或按需加载）——**那要另开需求**，因为它动的是「零外部依赖」这条
/// 交付纪律 —— 就**必须同时删掉** `script-src` / `style-src` 里的 `'unsafe-inline'`，
/// 改为 `'self'` 加载外部文件（若确需内联片段，用 nonce / hash，而不是重新放开
/// `'unsafe-inline'`）。⛔ 不许「拆了外部文件还把 `'unsafe-inline'` 留着」：
/// 那样既不内联、又放弃了 CSP 对内联注入的拦截，是两头都输。
///
/// `default-src 'self'` 是 AC-011 的下限；`connect-src 'self'` 把「页面被改成打
/// 第三方」变成一条会当场失败的请求（也解释了查询台 / 目录浏览为什么必须与
/// 控制面同源）。
pub const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; \
base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// 页面响应必须带的三个头（AC-011）。
///
/// 返回 `[(&'static str, &'static str); 3]` 而不是 `HeaderMap`：调用点是一个元组
/// 响应，写死成常量数组之后，「漏一个头」在 `tests/test_req125_page.rs` 里逐项红。
pub const SECURITY_HEADERS: [(&str, &str); 3] = [
    ("content-security-policy", CSP),
    ("x-content-type-options", "nosniff"),
    ("referrer-policy", "no-referrer"),
];

/// 把 [`SECURITY_HEADERS`] 变成 axum 的头部数组（类型转换只在这一处）。
pub fn security_headers() -> [(HeaderName, HeaderValue); 3] {
    [
        (
            HeaderName::from_static(SECURITY_HEADERS[0].0),
            HeaderValue::from_static(SECURITY_HEADERS[0].1),
        ),
        (
            HeaderName::from_static(SECURITY_HEADERS[1].0),
            HeaderValue::from_static(SECURITY_HEADERS[1].1),
        ),
        (
            HeaderName::from_static(SECURITY_HEADERS[2].0),
            HeaderValue::from_static(SECURITY_HEADERS[2].1),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 壳里不许出现任何**外部资源**引用（AC-002）。
    #[test]
    fn the_shell_has_no_external_dependency() {
        for needle in [
            "<script src=",
            "<link ",
            "href=",
            "https://",
            "@import",
            "integrity=",
            "http://",
        ] {
            assert!(
                !CONSOLE_HTML.contains(needle),
                "页面壳里出现了外部引用 {needle:?}（零依赖是 AC-002）"
            );
        }
    }

    /// 壳里不许出现 XSS 的两种常见写法（AC-010）。
    #[test]
    fn the_shell_never_uses_innerhtml_or_document_write() {
        for needle in ["innerHTML", "outerHTML", "document.write", "insertAdjacentHTML", "eval("] {
            assert!(
                !CONSOLE_HTML.contains(needle),
                "页面壳里出现了 {needle:?} —— 用户数据只能走 textContent"
            );
        }
    }

    /// 口令只进 sessionStorage（AC-003）：⛔ 长期存储、⛔ URL。
    #[test]
    fn the_shell_only_uses_session_storage_for_the_token() {
        assert!(
            !CONSOLE_HTML.contains("localStorage")
                && !CONSOLE_HTML.contains("indexedDB")
                && !CONSOLE_HTML.contains("document.cookie"),
            "口令只能存 sessionStorage"
        );
        assert!(CONSOLE_HTML.contains("sessionStorage"), "页面必须用 sessionStorage 记住口令");
        // URL 状态：页面不发任何带 token 的查询串，也不读 location.search。
        assert!(!CONSOLE_HTML.contains("location.search"));
        assert!(!CONSOLE_HTML.contains("location.hash"));
        assert!(!CONSOLE_HTML.contains("token="), "token 不许出现在 URL 里");
    }

    /// 壳是**零数据**的：不含任何具体项目 / 版本 / 对象 / 表名。
    #[test]
    fn the_shell_carries_no_data() {
        // 真实演示数据与判据用的假数据都不许在壳里。
        for needle in [
            "anspire",
            "secretproj",
            "绝密对象",
            "第 7 版",
            "customers",
            "deductions",
            "REPLACE-WITH",
            "admin-tokens",
        ] {
            assert!(
                !CONSOLE_HTML.contains(needle),
                "页面壳里出现了数据 {needle:?}（AC-001：壳必须零数据）"
            );
        }
    }

    /// 六个视图都在（AC-004）。
    ///
    /// REQ-161b：原先的「查询台」「目录浏览」两个视图整段移除（它们打的是 `query serve`，
    /// 不是本地操作），所以 8 → 6。**视图少了两项，但每一项都得在** —— 这条判据没有放松。
    #[test]
    fn the_shell_has_all_views() {
        for needle in [
            "项目与版本",
            "构建包",
            "评审",
            "导出",
            "声明编辑",
            "审计",
        ] {
            assert!(CONSOLE_HTML.contains(needle), "页面缺视图 {needle:?}");
        }
        // REQ-161b：这两个视图已删除 —— 判据反过来钉住「不许回来」。
        // 判的是**视图标题**（`<h2>`），不是「这几个字出现过」：删除处的注释里
        // 当然会写「查询台 / 目录浏览 已于…移除」，那正是要留给人看的原因。
        for gone in ["<h2>查询台</h2>", "<h2>目录浏览</h2>"] {
            assert!(
                !CONSOLE_HTML.contains(gone),
                "{gone} 已按 REQ-161b 移除（非本地操作），⛔ 别加回来"
            );
        }
        for tab in ["projects", "bundles", "review", "export", "config", "audit"] {
            assert!(
                CONSOLE_HTML.contains(&format!("id=\"v-{tab}\"")),
                "缺视图容器 v-{tab}"
            );
            assert!(
                CONSOLE_HTML.contains(&format!("id=\"tab-{tab}\"")),
                "缺 tab 按钮 tab-{tab}"
            );
        }
        // REQ-161b：视图容器与导航按钮各 6 个，且被删的两个不再留空壳。
        // （「别加回来」的逐条判据在 `tests/test_req125_page.rs::ac004`：那边点名那三条非本地端点。）
        assert_eq!(
            CONSOLE_HTML.matches("<section id=\"v-").count(),
            6,
            "视图容器必须恰好 6 个（查询台 / 目录浏览已移除）"
        );
        assert_eq!(
            CONSOLE_HTML.matches("data-tab=\"").count(),
            6,
            "导航按钮必须恰好 6 个（多一个就是有人把删掉的视图加回来了）"
        );
    }

    /// 构建包 tab 的 DOM 契约 + 请求契约（REQ-159）。
    ///
    /// 这些 id / 函数名是**对外契约**（QA 按 id 写测试、简报按函数名 grep），
    /// 改名字等于改接口，必须同时改这里。
    #[test]
    fn the_bundles_view_declares_its_contract() {
        for needle in [
            // 端点（逐字：服务端并行实现的就是这几条）
            "/bundles",
            "/database/test",
            "/key",
            "key/rotate",
            // 建项目 + 发 key：路径作用域 :p、key_label 必填（审计归属）
            "/api/projects",
            "key_label",
            "id=\"b-new-project\"",
            "id=\"b-new-key-label\"",
            "id=\"b-new-submit\"",
            // 激活成功后的固定警示：删了它，用户会拿着新 key 去查、拿到 503/404 再来问
            "P1b",
            "query serve 仍绑定单一项目",
            // 与 P1a 真实行为对齐的两条：重复 import 是幂等 200（不是 409）、am/ 只校验不落库
            "already_imported",
            "P1b 生效",
            // 组件计数必须按契约字段给摘要（entity_types / link_types / entities / sources），
            // 不许退回通用 count：缺字段就省略那一段，别整列掉成「—」。
            "entity_types",
            "link_types",
            "entities",
            "sources",
            "countSummary",
            "dsnFlag(",
            // key 能再次查看 ⇒ 每次查看都要留痕：审计提示是这条需求的唯一补偿，不许删
            "reveal_key",
            "id=\"b-key-view\"",
            // 上传必须是原始字节，不能走会 JSON 化的 post()
            "application/octet-stream",
            "ctype",
            // 目标库面板：口令框只支持新密码、不回填
            "b-dsn-pass",
            "new-password",
            // 门禁与 key 展示
            "id=\"b-import\"",
            "id=\"b-activate\"",
            "id=\"b-rotate\"",
            "id=\"b-key-copy\"",
            "待确认 ",
            "其中阻断 ",
            // 汇总口径**必须**取服务端聚合：自己数 items 会把「只签了一轨」漏掉
            "progress.pending",
            "progress.vetoed",
            "it.biz === \"pending\" || it.model === \"pending\"",
            // ⭐ REQ-161b：构建包是**本机主入口**，不是「历史遗留」——
            // 视图头必须把六步流程写全，ADR-013 只在底部留一行中性说明。
            "本机 FDE 操作：建项目 → 上传包 → 配目标库（需要时）→ 解析入库 → 待确认 → 生效。",
            "客户侧平台（Studio）另有同一套流程（ADR-013）；本页是 FDE 本机入口。",
            "不依赖任何外部服务",
        ] {
            assert!(
                CONSOLE_HTML.contains(needle),
                "构建包视图缺少 {needle:?}（REQ-159 的契约）"
            );
        }
        // REQ-161b：把「已迁走、别在这用」的措辞删掉 —— 用户看到那条黄条以为这功能没了。
        for gone in ["仅为历史保留", "请在 Studio 里做", "已迁到 Studio", "⚠️ 此流程已迁到"] {
            assert!(
                !CONSOLE_HTML.contains(gone),
                "构建包视图里不许再有 {gone:?}（REQ-161b：它就是本机主入口）"
            );
        }
        for fun in [
            "loadBundles()",
            "createProject()",
            "uploadBundle()",
            "showBundle(",
            "needDsn(",
            "testDsn()",
            "saveDsn()",
            "importBundle()",
            "loadChecklist()",
            "activateBundle()",
            "viewKey()",
            "showKey(",
            "rotateKey()",
        ] {
            assert!(
                CONSOLE_HTML.contains(fun),
                "缺函数 {fun:?}（简报按函数名 grep，名字不许改）"
            );
        }
        // 待确认清单只汇总 + 门禁：⛔ 这个 tab 里不许出现裁决请求，
        // 否则「评审」tab 与「构建包」tab 会有两套裁决入口。
        // （段号是**左导航序号**，构建包的 JS 块留在文件末尾，所以这一刀仍然只切到它自己。）
        let bundles_at = CONSOLE_HTML.find("// 2 · 构建包").expect("缺构建包 JS 段");
        assert!(
            !CONSOLE_HTML[bundles_at..].contains("api/decide"),
            "构建包 tab 不许自己调 /api/decide（裁决留在评审 tab）"
        );
    }

    /// 端点还不存在时的防御线：404 ⇒ 「服务端未就绪」，且**不当口令错**（REQ-159）。
    #[test]
    fn the_bundles_endpoints_fail_soft_when_the_server_is_not_ready() {
        assert!(
            CONSOLE_HTML.contains("服务端未就绪"),
            "404 必须显示一行「服务端未就绪」的人话"
        );
        assert!(
            CONSOLE_HTML.contains("ENDPOINT_NOT_READY"),
            "404 要有一个可辨认的错误码（便于与口令错区分）"
        );
        let not_ready = CONSOLE_HTML
            .find("opts.notReady && r.status === 404")
            .expect("缺 notReady 的 404 分支");
        let logout_call = CONSOLE_HTML
            .find("logout(\"口令无效")
            .expect("缺 401/403 的登出点");
        assert!(
            not_ready < logout_call,
            "404 分支必须排在「401/403 清口令」之前：两者互斥，404 绝不登出"
        );
        // 上传发的是文件句柄（原始字节），不是 JSON 字符串。
        assert!(
            CONSOLE_HTML.contains("body: f, ctype: \"application/octet-stream\""),
            "上传必须把文件句柄直接当 body 发（⛔ 不走 post()）"
        );
    }

    /// 「组件没有 `requires_dsn` 字段 ⇒ 显示 `—`」（⛔ 不许渲染成「否」）。
    ///
    /// 为什么值得一条判据：把**字段不存在**渲染成**具体值**（「否」），等于替服务端下了一个
    /// 它没下的结论。这类漂移只有上真浏览器才看得见（QA 的 Playwright 就是这么抓到组件计数
    /// 那整列「—」的），所以把它钉在源码上：`dsnFlag()` 必须先判字段缺省，再落到 `flagOf`。
    #[test]
    fn a_missing_requires_dsn_renders_as_dash_not_false() {
        let at = CONSOLE_HTML
            .find("function dsnFlag(c)")
            .expect("缺 dsnFlag（requires_dsn 列的渲染点）");
        let tail = &CONSOLE_HTML[at..];
        let end = tail
            .find("\n}")
            .map(|n| n + 2)
            .expect("dsnFlag 没有函数体结尾");
        let body = &tail[..end];
        assert!(
            body.contains("c.requires_dsn === undefined"),
            "dsnFlag 必须先判「组件没有这个字段」：{body}"
        );
        assert!(
            body.contains("return \"—\""),
            "字段不存在时必须返回 —（⛔ 不是「否」）：{body}"
        );
        let dash_at = body.find("return \"—\"").unwrap();
        let flag_at = body
            .find("flagOf(")
            .expect("dsnFlag 里要保留真正的 是/否 分支（否则「是」也永远不显示）");
        assert!(
            dash_at < flag_at,
            "「—」分支必须排在 flagOf 之前：先判缺省，再下结论。{body}"
        );
        // 反向自证：组件表的 requires_dsn 列确实走这个函数（不是写了个没人调的函数）。
        assert!(
            CONSOLE_HTML.contains("dsnFlag(c), c ? (c.dialect"),
            "组件表的 requires_dsn 列必须走 dsnFlag()"
        );
    }

    /// REQ-161b：查询台移除后，**不许**有任何回执元信息渲染残留（AC-008 随视图一起退休）。
    ///
    /// 原判据（`the_query_view_declares_all_four_meta_fields`）钉的是查询台必须显示
    /// `warnings / coverage / join_states / truncated` 四段。视图本身就是这次要删的东西
    /// （它打的是 `query serve`，不是本地操作），所以判据反过来：这四段的渲染点
    /// 一处都不许留在本页 —— 否则就是「视图删了、代码还在偷偷跑」。
    #[test]
    fn the_removed_query_view_leaves_no_meta_rendering_behind() {
        for gone in ["function metaLines(", "renderQueryMeta(", "renderQueryTable(", "function runQuery("] {
            assert!(
                !CONSOLE_HTML.contains(gone),
                "{gone} 是查询台的渲染点，随 REQ-161b 一起删（视图没了，逻辑也别留）"
            );
        }
        // `warnings` 例外：构建包的回执 warnings[] 仍在用（AM 只校验不落库那条），
        // 所以只钉「查询台独有的那两段」+ 四个函数。
        for gone in ["join_states", "truncated"] {
            assert!(
                !CONSOLE_HTML.contains(gone),
                "查询回执的 {gone:?} 处理已随查询台移除（REQ-161b）"
            );
        }
    }

    /// 五个危险动作都过 `guarded`，且确认框写明**目标 + 后果**（AC-006）。
    ///
    /// 现在是 5 处：apply / freeze / 声明保存（REQ-125 的三处）
    /// + 构建包激活 / 换 key（REQ-159 的两处）。
    #[test]
    fn the_five_dangerous_actions_are_guarded() {
        // `function guarded(spec, run)` 不命中 `guarded({` ⇒ 恰好 5 处调用。
        assert_eq!(
            CONSOLE_HTML.matches("guarded({").count(),
            5,
            "只有 apply / freeze / 声明保存 / 激活 / 换 key 五处危险动作（多了少了都要重看）"
        );
        assert_eq!(
            CONSOLE_HTML.matches("[\"目标\"").count(),
            5,
            "每个确认框都要写目标"
        );
        assert_eq!(
            CONSOLE_HTML.matches("[\"后果\"").count(),
            5,
            "每个确认框都要写后果"
        );
    }

    /// 项目名允许**下划线**（F-1）：前端校验必须与 `manifest.project` 的 pattern 同一口径。
    ///
    /// 钉两件事：
    /// 1. 字符类里有 `_`（⛔ 别退回旧的 `[a-z0-9-]`，那会把 `ops_team` 判成非法）；
    /// 2. 校验只有**一个**实现点 `projectNameOk()`，`createProject()` 必须调它 ——
    ///    否则「代码放行、文案拦下」或反过来，用户看到的就是自相矛盾的报错。
    ///
    /// 大写 / 空格 / 斜杠 / 点仍然必须被拒（`A-Z` 不许回到字符类里）。
    #[test]
    fn the_project_name_pattern_allows_underscore() {
        assert!(
            CONSOLE_HTML.contains("a-z0-9_-"),
            "项目名 pattern 必须允许下划线（首字符字母或数字，其后可含 _）"
        );
        assert!(
            !CONSOLE_HTML.contains("a-z0-9-]"),
            "别留着旧字符类 [a-z0-9-]：那会把带下划线的项目名判成非法"
        );
        assert!(
            CONSOLE_HTML.contains("function projectNameOk(name)"),
            "项目名校验要有唯一实现点 projectNameOk()"
        );
        assert!(
            CONSOLE_HTML.contains("!projectNameOk(name)"),
            "createProject() 必须走 projectNameOk()（⛔ 别再加一条内联正则）"
        );
        let at = CONSOLE_HTML
            .find("function projectNameOk(name)")
            .expect("缺 projectNameOk()");
        let body = &CONSOLE_HTML[at..(at + 240).min(CONSOLE_HTML.len())];
        assert!(!body.contains("A-Z"), "大写字母必须继续被拒：{body}");
        assert!(body.contains("_"), "字符类里必须有下划线：{body}");
        // 代码与文案一起改：报错 / 说明里都要提到下划线，否则「说得比做的严」。
        assert!(
            CONSOLE_HTML.contains("连字符 / 下划线"),
            "报错与说明文案必须与 pattern 同步提到下划线"
        );
    }

    // ══════════════════════════════════════════════════════════
    // REQ-161 · 换皮：左导航 / 密度 / 等宽 key / 端点集合冻结
    // （REQ-161b 收口：8 视图 → 6，目录抽屉随视图移除）
    // ══════════════════════════════════════════════════════════

    /// 左导航 + 单一主区（AC-001）。
    ///
    /// 换皮前是顶栏 8 个并列 tab（`#tabs`）；换皮后是左侧窄导航 + 单一主区。
    /// REQ-161b 之后是 **6 个入口**，且**顺序有意义**：01 项目与版本 / 02 构建包 /
    /// 03 评审 / 04 导出 / 05 声明编辑 / 06 审计 —— 构建包是用户每天第一件事（上传包），
    /// 所以在第 2 位（原来排最后，用户以为没有这个功能）。
    #[test]
    fn the_shell_uses_a_left_nav_with_all_six_entries() {
        assert!(CONSOLE_HTML.contains("id=\"nav\""), "缺左导航容器 #nav");
        assert!(CONSOLE_HTML.contains("id=\"nav-list\""), "缺导航列表 #nav-list");
        assert!(
            CONSOLE_HTML.contains("id=\"views\""),
            "缺单一主区滚动容器 #views（左导航 + 单主区是这次换皮的结构）"
        );
        assert!(
            !CONSOLE_HTML.contains("id=\"tabs\""),
            "顶栏 #tabs 必须移除：入口已经搬到左导航（AC-001）"
        );
        assert_eq!(
            CONSOLE_HTML.matches("class=\"nav-item").count(),
            6,
            "左导航必须恰好 6 个入口（项目与版本/构建包/评审/导出/声明编辑/审计）"
        );
        for (tab, num, title) in [
            ("projects", "01", "项目与版本"),
            ("bundles", "02", "构建包"),
            ("review", "03", "评审"),
            ("export", "04", "导出"),
            ("config", "05", "声明编辑"),
            ("audit", "06", "审计"),
        ] {
            assert!(
                CONSOLE_HTML.contains(&format!("id=\"tab-{tab}\" class=\"nav-item")),
                "左导航缺入口 {title}"
            );
            assert!(
                CONSOLE_HTML.contains(&format!("data-tab=\"{tab}\"")),
                "{title} 缺 data-tab={tab}（导航切换靠它）"
            );
            assert!(
                CONSOLE_HTML.contains(&format!(">{num}</span>{title}</button>")),
                "{title} 的序号 / 文案与样机不一致（应为 {num} {title}）"
            );
        }
        // 分组：建库（01）/ 入库（02）/ 签与交付（03–06）；「查询」整组消失。
        for group in ["建库", "入库", "签与交付"] {
            assert!(
                CONSOLE_HTML.contains(&format!("<div class=\"nav-group\">{group}</div>")),
                "导航分组缺 {group:?}"
            );
        }
        assert!(
            !CONSOLE_HTML.contains(">运维</div>") && !CONSOLE_HTML.contains(">查询</div>")
                && !CONSOLE_HTML.contains(">构建</div>"),
            "旧分组（运维 / 查询 / 构建）必须消失（REQ-161b 只留三组）"
        );
        // 顺序：02 就是构建包（按 id 在文档里的先后判，别只判「存在」）。
        let bar = CONSOLE_HTML.find("id=\"tab-bundles\"").expect("缺构建包入口");
        let bar_icon = &CONSOLE_HTML[bar..];
        assert!(
            bar_icon.contains(">02</span>构建包"),
            "构建包必须是 02（第 2 位）：用户第一件事是把包传上来"
        );
        let review = CONSOLE_HTML.find("id=\"tab-review\"").expect("缺评审入口");
        assert!(bar < review, "构建包必须排在评审之前（01 → 02 → 03…）");
    }

    /// REQ-161b：目录浏览视图已移除 ⇒ **右侧详情面板（抽屉）一并移除**。
    ///
    /// 原判据（目录详情是默认收起的右侧抽屉）已随视图删除：抽屉只为目录详情服务，
    /// 视图没了它就没有存在意义。这里反过来钉住「别留半个」：面板容器、标题、关闭按钮、
    /// 滑出的 CSS 依据，一处都不许剩。
    #[test]
    fn the_removed_browse_view_leaves_no_detail_panel_behind() {
        for gone in [
            "id=\"dr-close\"",
            "id=\"dr-title\"",
            "id=\"k-detail\"",
            "transform:translateX(102%)",
        ] {
            assert!(
                !CONSOLE_HTML.contains(gone),
                "{gone} 属于目录浏览的详情面板，随 REQ-161b 一起移除"
            );
        }
        // 更彻底的「DOM 里没有那块面板」判据（含 id / 遮罩 / 查询服务的三条错误分支）在
        // `tests/test_req125_page.rs::ac004b_the_removed_views_leave_no_detail_panel` ——
        // 那边点名，这里只钉元素与样式。
    }

    /// key / IRI 一律等宽（`.key`），**三处**必须覆盖：审计 / 评审 / 项目与版本（AC-002）。
    ///
    /// REQ-161b：原来抽查的是「目录浏览 / 审计 / 评审」；目录视图删了 ⇒ 换成**项目与版本**
    /// 的项目名（它进 `:p` 路径，是页面里最像 key 的东西）。⛔ 别让这条判据空过：
    /// 三处必须各有一个**具体渲染点**，不能只留 CSS 定义。
    #[test]
    fn key_and_iri_are_monospace_in_audit_review_and_projects() {
        assert!(
            CONSOLE_HTML.contains(".key{font-family:var(--mono)"),
            "CSS 必须定义 .key 等宽类"
        );
        // 项目与版本：项目名（GET /api/projects 的 project）。
        assert!(
            CONSOLE_HTML.contains("el(\"b\", \"key\", d.project)"),
            "项目与版本的项目名必须带 .key"
        );
        // 审计：token 摘要与动作（靠视觉模式识别 sha 前缀 / action）。
        assert!(
            CONSOLE_HTML.contains("{ text: e.token || \"—\", cls: \"key\" }"),
            "审计的 token 摘要必须带 .key"
        );
        assert!(
            CONSOLE_HTML.contains("{ text: e.action || \"—\", cls: \"key\" }"),
            "审计的动作必须带 .key"
        );
        // 评审：条目 target（对象名 / 关系名 / IRI）。
        assert!(
            CONSOLE_HTML.contains("el(\"span\", \"target key\", it.target)"),
            "评审的 target 必须带 .key"
        );
        // 反向自证：目录那一套（可点名字 `.keylink`）随视图一起没了，别留半套样式。
        assert!(
            !CONSOLE_HTML.contains(".keylink"),
            "目录浏览的 .keylink 已随视图移除（REQ-161b）"
        );
    }

    /// 表格是**内部工具**的密度：表头 sticky、行高 32px（AC-002）。
    ///
    /// 这两条只能靠 CSS 源码断言（进程内不起浏览器）；真浏览器里是否生效由 BA 的 Playwright harness 看。
    #[test]
    fn tables_are_dense_with_a_sticky_header() {
        assert!(
            CONSOLE_HTML.contains("thead th{position:sticky;top:0"),
            "表头必须 sticky（滚动时不丢列名）"
        );
        assert!(
            CONSOLE_HTML.contains("tbody td{height:32px"),
            "表格行高必须是 32px（一屏 20+ 行）"
        );
        assert!(
            CONSOLE_HTML.contains("class=\"tablewrap"),
            "表体得有滚动容器（sticky 才有参照物）"
        );
        // REQ-161b：原来指向目录表（已删）⇒ 改指**审计表**（页面上最长的那张）。
        assert!(
            CONSOLE_HTML.contains("id=\"a-table\" class=\"tablewrap tall\""),
            "审计表 #a-table 必须在 .tablewrap 里（sticky 表头 + 32px 行高就落在这张表上）"
        );
    }

    /// 端点集合冻结（AC-004）：页面里出现的 `/api/…` 路径集合必须逐字等于清单。
    ///
    /// 提取口径：从 `api/` 起、由小写字母 / 数字 / `_` `.` `/` `-` 组成的路径，
    /// 去掉尾斜杠后去重（`api/projects/` 与 `api/projects` 视为同一条基路径）。
    /// 清单是**本单开工前**从 `CONSOLE_HTML` 里跑出来的；改一个都得先回答「为什么端点变了」。
    ///
    /// REQ-161b：12 → **9**。查询服务的三条读端点（原第 3 / 6 / 12 项）随两个视图一起删除：
    /// 它们属于 `query serve`（另一个进程），不是本地操作。构建包的 `bundles` 系列是
    /// **动态拼接**（`bundlePath` 里的 `"/bundles"`），仍在下列路径里由 `api/projects` 承载，
    /// ⛔ 别把它当「多余的」删掉。
    #[test]
    fn the_api_path_set_is_frozen() {
        const FROZEN: [&str; 9] = [
            "api/apply",
            "api/audit",
            "api/config",
            "api/decide",
            "api/export/ttl",
            "api/freeze",
            "api/projects",
            "api/r2rml",
            "api/review",
        ];
        let mut found: Vec<String> = Vec::new();
        let mut rest: &str = CONSOLE_HTML;
        while let Some(at) = rest.find("api/") {
            let tail = &rest[at..];
            let end = tail
                .find(|c: char| {
                    !(c.is_ascii_lowercase()
                        || c.is_ascii_digit()
                        || c == '_'
                        || c == '/'
                        || c == '.'
                        || c == '-')
                })
                .unwrap_or(tail.len());
            let path = tail[..end].trim_end_matches('/').to_string();
            if path != "api" && !found.contains(&path) {
                found.push(path);
            }
            rest = &rest[at + 4..];
        }
        found.sort();
        let mut want: Vec<String> = FROZEN.iter().map(|s| (*s).to_string()).collect();
        want.sort();
        assert_eq!(
            found, want,
            "页面里的 /api/* 路径集合变了：换皮只许动壳，端点集合必须与本单之前逐字相等（AC-004）"
        );
    }

    /// 体积预算（S6）：单文件 ≤ 300 KB；下限自证页面没被截断。
    ///
    /// 上界比 `tests/test_req125_page.rs::ac002`（< 200 KB）更宽，那是另一道更严的闸；
    /// 这条按需求 S6 的数字登记，防止「换皮顺手塞了几个内联图片」这种膨胀。
    #[test]
    fn the_shell_stays_under_the_size_budget() {
        let bytes = CONSOLE_HTML.len();
        assert!(
            bytes <= 300 * 1024,
            "单文件必须 ≤ 300 KB（S6），实际 {bytes} 字节"
        );
        assert!(bytes > 80 * 1024, "页面太小，可能被截断了：{bytes} 字节");
    }

    /// REQ-161b：构建包是**本机主入口**，ADR-013 只许作为底部一行中性说明存在。
    ///
    /// 用户的原话是「我没有看到构建包的操作」—— 因为顶上压着一条黄条写着「已迁到 Studio…
    /// 仅为历史保留，新流程请在 Studio 里做」，读起来像「这功能废弃了」。所以判据从
    /// 「提示必须留着」翻成四条：
    /// 1. 视图头把六步流程写全（建项目 → 上传 → 配目标库 → 解析入库 → 待确认 → 生效）；
    /// 2. ADR-013 那句压成**一行** `.note`，不再带 ⚠️、不再带警示色；
    /// 3. ⛔ 不许出现「仅为历史保留 / 请在 Studio 里做」这类把人赶走的措辞；
    /// 4. 构建包视图本身当然还在（六步面板一个不少）。
    #[test]
    fn the_adr013_studio_line_is_a_footnote_not_a_warning() {
        assert!(
            CONSOLE_HTML.contains(
                "本机 FDE 操作：建项目 → 上传包 → 配目标库（需要时）→ 解析入库 → 待确认 → 生效。"
            ),
            "构建包视图头必须写清六步流程（用户要一眼看到「这功能在」）"
        );
        assert!(
            CONSOLE_HTML.contains("全部走本机 console"),
            "视图头必须写明「全部走本机 console」（用户原话：我们的返回都是本地操作）"
        );
        assert!(
            CONSOLE_HTML.contains(
                "<p class=\"note\">客户侧平台（Studio）另有同一套流程（ADR-013）；本页是 FDE 本机入口。</p>"
            ),
            "ADR-013 只能作为底部一行中性说明（.note），⛔ 不是警示框"
        );
        for gone in ["仅为历史保留", "请在 Studio 里做", "已迁到 Studio"] {
            assert!(
                !CONSOLE_HTML.contains(gone),
                "{gone:?} 必须删除：它就是让用户以为「没有构建包操作」的那句话"
            );
        }
        assert!(
            CONSOLE_HTML.contains("id=\"v-bundles\""),
            "构建包视图本身必须留着（它就是本机主入口）"
        );
        // 六步面板的锚点还在（1 上传 / 2 目标库 / 3 解析入库 / 4 待确认 / 5 生效）。
        for step in ["1 · 上传并校验", "3 · 解析入库", "4 · 待确认清单", "5 · 生效"] {
            assert!(CONSOLE_HTML.contains(step), "构建包缺步骤 {step:?}");
        }
    }

    /// `POST /api/r2rml` 回的是 **text/turtle**，页面必须按文本取正文（REQ-161 追加修复）。
    ///
    /// 基线缺陷（不是换皮引入，但长在「导出」视图里）：`exportR2rml()` 当时走 `post()`
    /// ⇒ `json()` ⇒ `JSON.parse` 一份 Turtle ⇒ **必抛 `BAD_JSON`**，还把「服务端返回的
    /// 是 Turtle」说成「去看服务端日志」—— FDE 点一次「生成 R2RML」就永远失败。
    ///
    /// 钉四件事：
    /// 1. `postText()` 存在**且真被调用**（写了没人调 = 没修）；
    /// 2. `exportR2rml()` 里**不再**出现 `post("api/r2rml"`（回退即红）；
    /// 3. `postText` 别用过头：apply / decide 仍是 JSON 端点（REQ-161b：查询台删除后只剩这两条）；
    /// 4. 别顺手把 TTL 也改了（它本来就是文本路径，走 `request()` 的 `r.text`）。
    #[test]
    fn r2rml_is_read_as_text_not_parsed_as_json() {
        assert!(
            CONSOLE_HTML.contains("function postText(path, body)"),
            "缺 postText()：文本端点要有一个不走 JSON.parse 的发包口"
        );
        let at = CONSOLE_HTML
            .find("function exportR2rml()")
            .expect("缺 exportR2rml()");
        // 取到这个函数体结尾（按行首的 `}` 切）—— 别用固定字节数：中文注释会切在字符中间。
        let tail = &CONSOLE_HTML[at..];
        let end = tail
            .find("\n}")
            .map(|n| n + 2)
            .expect("exportR2rml 没有函数体结尾");
        let body = &tail[..end];
        assert!(
            body.contains("postText(\"api/r2rml\""),
            "exportR2rml() 必须走 postText()：服务端回的是 text/turtle"
        );
        assert!(
            !body.contains("post(\"api/r2rml\""),
            "⛔ 别再走 post()：它会把 Turtle 当 JSON 解析 ⇒ 必抛 BAD_JSON"
        );
        // ⛔ postText 不许用过头：这两条端点回的确实是 JSON。
        for needle in ["post(\"api/apply\"", "post(\"api/decide\""] {
            assert!(
                CONSOLE_HTML.contains(needle),
                "JSON 端点 {needle:?} 必须继续走 post()/json()"
            );
        }
        // ⛔ TTL 本来就是文本路径，别顺手改。
        assert!(
            CONSOLE_HTML.contains("request(q).then(function (r) {"),
            "TTL 走 request() 取正文（不经 postText / json）"
        );
    }
}
