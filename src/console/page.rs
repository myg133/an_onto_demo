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
//! | `GET /` 的响应体（本文件的 [`CONSOLE_HTML`]） | **只有**界面骨架：标题、tab 名、表单、CSS/JS |
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
//! # 八个视图消费的端点（⛔ 本文件没有、也不许有新的数据端点）
//!
//! | 视图 | 端点 |
//! | --- | --- |
//! | 项目与版本 | `GET /api/projects` · `POST /api/apply` · `POST /api/freeze` |
//! | 评审 | `GET /api/review` · `POST /api/decide` |
//! | 导出 | `GET /api/export/ttl` · `POST /api/r2rml` |
//! | 声明编辑 | `GET/PUT /api/config/{exclusions,policy,tenancy}` |
//! | 审计 | `GET /api/audit` |
//! | 查询台 | `POST /api/sparql`（⭐ REQ-142：请求体显式带 `format:"rows"` —— 服务默认是 JSON-LD 图） |
//! | 目录浏览 | `GET /api/catalog` · `GET /api/describe` |
//! | 构建包（REQ-159） | `GET /api/projects` · `POST /api/projects/<p>/bundles` · `PUT /api/projects/<p>/database` · `POST /api/projects/<p>/database/test` · `POST /api/projects/<p>/bundles/<id>/import` · `GET /api/review?proposal=` · `POST /api/projects/<p>/bundles/<id>/activate` · `POST /api/projects/<p>/key/rotate` |
//!
//! 前四组由 `console serve` 自己提供；查询台与目录浏览是 **REQ-123 的查询服务**
//! （`ontok query serve` 的 `/api/sparql` / `/api/catalog` / `/api/describe`），
//! 本页只按**同源**路径调用（`connect-src 'self'`）。⇒ 部署时把控制面与查询服务
//! 放在**一个源**后面（反向代理按路径分流）；直连两个端口时浏览器会用同源策略拦掉，
//! 页面会**明确**报「查询服务未接入」+ 接法（本机 `tests/fixtures/req125/dev_proxy.ps1`），
//! 而不是把 404/403 说成「口令不对」——那两个状态在这条拓扑下**不是口令问题**，
//! 所以查询端点出错时页面**不清任何口令**（清掉就是死循环）；管理端点仍按 401/403
//! 清口令回登录态。开发期用 `tests/fixtures/req125/dev_proxy.ps1`（同一个分流口径）。
//! 最后一组（构建包）由 **REQ-159 的服务端**提供，见下一节。
//!
//! # 构建包 tab：端点不存在时先说「服务端未就绪」，⛔ 不把它当口令错
//!
//! 构建包这一组端点（`/bundles…` / `/database…` / `/key/rotate`）在页面上线时
//! **可能还不存在**（服务端并行实现）。所以这一 tab 的每一次请求都带 `notReady`
//! 标记：HTTP **404** ⇒ 在对应区域显示一行
//! 「服务端未就绪：该端点还不存在（REQ-159 服务端在并行开发中）」，
//! 并且**不登出、不清口令** —— 404 在这里是「服务端还没这个路径」，不是「口令不对」。
//! 只有 **401 / 403** 才走既有的清口令 + 回登录态（安全语义不变）。
//!
//! 两条只在本 tab 成立的纪律：
//!
//! | 规则 | 为什么 |
//! | --- | --- |
//! | key **只放内存**（`LAST_KEY`），⛔ 不进 sessionStorage / 任何输入框 | 「只显示一次」的反面是不许留痕；写下来就不再是一次性 |
//! | DSN 口令框保存成功后**只清 value**，不回显、不记录 | 页面没有回读 DSN 的端点，回显等于凭空造一份明文 |
//!
//! 上传走 `fetch` 的**原始字节**（`Content-Type: application/octet-stream`），
//! ⛔ 不能用 `post()`：它会把 body `JSON.stringify` 成字符串，文件字节当场写坏。
//! 即便如此，整页仍然**只有一个 `fetch(` 调用点**（`request()` 里的那一处，
//! 上传只是给它传 `body` + `ctype`），`tests/test_req125_page.rs::ac004` 的数法不变。
//!
//! # 查询台**必须**显示四件事（AC-008）
//!
//! `warnings`（可能漏数据）、`coverage[]`、`join_states[]` 里 `used=false` 的边、
//! `truncated` —— 只给结果表格 = 不合格。这四段由 [`CONSOLE_HTML`] 里的
//! `metaLines()` **一处**产出（纯函数：输入回执 JSON，输出要显示的文案 + 样式），
//! 于是「页面漏了一条警告」在 `tests/fixtures/req125/page_probe.js` 里当场红。
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
//! 评审 40 条/页、审计默认 100（服务端上限 1000，页面最多渲染 1000）、
//! 目录按服务端分页（`page_size` ≤ 200）、查询结果最多渲染 200 行并写明
//! 「只显示前 N 行 / 共 M 行」。⛔ 不一次渲染上万行。

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
:root{--bg:#141416;--fg:#f2f2f7;--mut:#9b9ba1;--line:#3c3c40;--card:#1c1c1f;--acc:#4d9bff;--ok:#3fbf6a;--bad:#ff6b5e;--warn:#ffb340;--head:#232327}
@media (prefers-color-scheme: light){
  :root{--bg:#fbfbfd;--fg:#1d1d1f;--mut:#6e6e73;--line:#d2d2d7;--card:#fff;--acc:#0a6cff;--ok:#0a8f3c;--bad:#c2261c;--warn:#b26a00;--head:#f2f2f5}
}
*{box-sizing:border-box}
body{margin:0;font:14px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Hiragino Sans GB","Microsoft YaHei",sans-serif;background:var(--bg);color:var(--fg)}
header{position:sticky;top:0;z-index:8;background:var(--head);border-bottom:1px solid var(--line);padding:8px 16px;display:flex;gap:10px;align-items:center;flex-wrap:wrap}
h1{font-size:15px;margin:0;font-weight:600;white-space:nowrap}
#who{color:var(--mut);font-size:13px}
#who b{color:var(--fg)}
input,select,textarea{font:inherit;color:var(--fg);background:var(--card);border:1px solid var(--line);border-radius:6px;padding:4px 8px}
input[type=password]{width:190px}
textarea{width:100%;min-height:260px;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:12px;line-height:1.5;padding:10px;resize:vertical}
button{font:inherit;padding:4px 11px;border:1px solid var(--line);border-radius:6px;background:var(--card);color:var(--fg);cursor:pointer;white-space:nowrap}
button:hover{border-color:var(--acc);color:var(--acc)}
button:disabled{opacity:.45;cursor:default}
button.danger{border-color:var(--bad);color:var(--bad)}
button.danger:hover{background:var(--bad);color:#fff}
button.ok{border-color:var(--ok);color:var(--ok)}
#msg{font-size:13px;flex:1;min-width:220px;color:var(--mut)}
#msg.bad{color:var(--bad);font-weight:600}
#msg.ok{color:var(--ok)}
#tabs{display:flex;gap:6px;flex-wrap:wrap;padding:8px 16px;border-bottom:1px solid var(--line);background:var(--card);position:sticky;top:49px;z-index:7}
#tabs button{border-radius:999px;padding:3px 13px;color:var(--mut)}
#tabs button.on{border-color:var(--acc);color:var(--acc);font-weight:600}
main{padding:14px 16px 80px;max-width:1280px;margin:0 auto}
.view[hidden]{display:none}
.bar{display:flex;gap:10px;align-items:center;flex-wrap:wrap;margin-bottom:10px}
.note{color:var(--mut);font-size:12px}
.panel{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:10px 12px;margin-bottom:12px}
.panel h2{font-size:13px;margin:0 0 8px;color:var(--fg);font-weight:600}
.grid2{display:grid;grid-template-columns:1fr 1fr;gap:12px}
@media (max-width:900px){.grid2{grid-template-columns:1fr}}
table{width:100%;border-collapse:collapse;font-size:13px}
th,td{text-align:left;padding:4px 8px;border-bottom:1px solid var(--line);vertical-align:top}
th{font-weight:600;font-size:12px;color:var(--mut);white-space:nowrap}
td.mono,.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:12px;word-break:break-all}
.badge{display:inline-block;font-size:12px;padding:0 7px;border-radius:999px;border:1px solid var(--line);color:var(--mut);white-space:nowrap}
.badge.ok{color:var(--ok);border-color:var(--ok)}
.badge.bad{color:var(--bad);border-color:var(--bad)}
.badge.warn{color:var(--warn);border-color:var(--warn)}
.badge.acc{color:var(--acc);border-color:var(--acc)}
ul.warns{margin:6px 0;padding-left:18px}
ul.warns li{margin:3px 0;color:var(--warn)}
ul.warns li.bad{color:var(--bad);font-weight:600}
#summary,.summary{color:var(--mut);font-size:13px;margin-bottom:10px}
#summary b,.summary b{color:var(--fg)}
article.item{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:8px 11px;margin-bottom:8px}
article.item.vetoed{border-color:var(--bad)}
.hd{display:flex;gap:9px;align-items:baseline;flex-wrap:wrap}
.idx{color:var(--mut);font-size:12px;font-variant-numeric:tabular-nums}
.target{font-weight:600}
.kind{font-size:12px;color:var(--mut);border:1px solid var(--line);border-radius:999px;padding:0 8px}
.why{font-size:12px;color:var(--mut);margin-top:4px}
.why b{color:var(--fg)}
.tracks{display:flex;gap:18px;flex-wrap:wrap;margin-top:6px;font-size:13px}
.track{display:flex;gap:6px;align-items:center}
.track .name{color:var(--mut)}
.st{font-size:12px;border:1px solid var(--line);border-radius:5px;padding:0 6px}
.st.accepted{color:var(--ok);border-color:var(--ok)}
.st.rejected{color:var(--bad);border-color:var(--bad)}
.st.pending{color:var(--mut)}
pre{margin:6px 0 0;padding:8px;background:var(--bg);border:1px solid var(--line);border-radius:6px;font-size:12px;overflow:auto;max-height:300px;white-space:pre-wrap;word-break:break-all}
.pager{display:flex;gap:10px;align-items:center;margin-top:10px;font-size:13px;color:var(--mut)}
.err{background:var(--card);border:1px solid var(--bad);border-radius:10px;padding:10px 12px;margin:10px 0}
.err .code{font-family:ui-monospace,Consolas,monospace;font-size:12px;color:var(--bad);font-weight:600}
.err .msg{margin-top:4px}
.err .hint{margin-top:4px;color:var(--mut);font-size:12px}
.okbox{background:var(--card);border:1px solid var(--ok);border-radius:10px;padding:10px 12px;margin:10px 0}
.kv{display:grid;grid-template-columns:160px 1fr;gap:2px 12px;font-size:13px}
.kv dt{color:var(--mut)}
.kv dd{margin:0;word-break:break-all}
#modal{position:fixed;inset:0;background:rgba(0,0,0,.55);display:flex;align-items:center;justify-content:center;padding:20px;z-index:30}
#modal[hidden]{display:none}
#mbox{background:var(--card);border:1px solid var(--line);border-radius:12px;max-width:640px;width:100%;max-height:82vh;overflow:auto;padding:16px 20px}
#mbox h2{margin:0 0 10px;font-size:16px}
#m-rows{display:grid;grid-template-columns:72px 1fr;gap:6px 10px;margin:0 0 12px;font-size:13px}
#m-rows dt{color:var(--mut)}
#m-rows dd{margin:0;word-break:break-all}
#mbox .acts{display:flex;gap:10px;justify-content:flex-end;margin-top:6px}
.empty{color:var(--mut);text-align:center;padding:30px 0}
</style>
</head>
<body>
<header>
  <h1>内部工作台</h1>
  <span id="who">未连接</span>
  <span id="login">
    <input id="token" type="password" placeholder="管理口令（admin token）" autocomplete="off">
    <input id="qtoken" type="password" placeholder="查询口令（query token，可选）" autocomplete="off">
    <button id="go">载入</button>
  </span>
  <span id="msg">口令只留在本标签页的 sessionStorage 里：不进地址栏、不写长期存储。</span>
</header>

<nav id="tabs">
  <button id="tab-projects" data-tab="projects" class="on">项目与版本</button>
  <button id="tab-review" data-tab="review">评审</button>
  <button id="tab-export" data-tab="export">导出</button>
  <button id="tab-config" data-tab="config">声明编辑</button>
  <button id="tab-audit" data-tab="audit">审计</button>
  <button id="tab-query" data-tab="query">查询台</button>
  <button id="tab-catalog" data-tab="catalog">目录浏览</button>
  <button id="tab-bundles" data-tab="bundles">构建包</button>
</nav>

<main>
  <!-- ── 1 · 项目与版本 ─────────────────────────────────── -->
  <section id="v-projects" class="view">
    <div class="bar">
      <button id="p-refresh">刷新</button>
      <span class="note">GET /api/projects —— 项目 / 版本 / 冻结状态 / 对象·关系计数；本服务只服务启动时指定的那一个项目。</span>
    </div>
    <div id="p-summary" class="summary">尚未载入。</div>
    <div id="p-table"></div>
    <div class="panel" style="margin-top:12px">
      <h2>应用提案（apply）</h2>
      <p class="note">apply 会把审核过的提案写成**新版本**（对象 / 关系落库）。已应用的提案再 apply ⇒ 409；基版本已冻结也 ⇒ 409。此动作不可撤销。</p>
      <div class="bar">
        <label>提案别名 <input id="p-alias" value="p1" size="10"></label>
        <button id="p-apply" class="danger">应用提案…</button>
      </div>
      <div id="p-apply-out"></div>
    </div>
  </section>

  <!-- ── 2 · 评审 ───────────────────────────────────────── -->
  <section id="v-review" class="view" hidden>
    <div class="bar">
      <label>提案 <input id="r-proposal" value="p1" size="8"></label>
      <button id="r-load">载入</button>
      <span class="note">GET /api/review?proposal=…（p1 = 该项目内最近导入的提案）；逐条签走 POST /api/decide。</span>
    </div>
    <div id="r-summary" class="summary">尚未载入。</div>
    <div id="r-list"></div>
    <div class="pager" id="r-pager"></div>
  </section>

  <!-- ── 3 · 导出 ───────────────────────────────────────── -->
  <section id="v-export" class="view" hidden>
    <div class="grid2">
      <div class="panel">
        <h2>TTL（GET /api/export/ttl）</h2>
        <p class="note">与 `ontok ontology export` 是同一份字节（sha256 应相等）。</p>
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
      <div class="panel">
        <h2>R2RML（POST /api/r2rml）</h2>
        <p class="note">与 `ontok r2rml generate` 是同一份字节；`include_inherit` 决定继承边要不要生成 join。</p>
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
  </section>

  <!-- ── 4 · 声明编辑 ───────────────────────────────────── -->
  <section id="v-config" class="view" hidden>
    <div class="bar">
      <label>声明
        <select id="c-kind">
          <option value="exclusions">exclusions（排除）</option>
          <option value="policy">policy（租户口径）</option>
          <option value="tenancy">tenancy（租户列标注）</option>
        </select>
      </label>
      <button id="c-load">载入</button>
      <button id="c-save" class="danger">保存…</button>
      <span class="note">PUT 是整份替换：服务端先按 schema 校验表 / 列，失败 ⇒ 400 且原文件一个字节都不变。</span>
    </div>
    <div id="c-meta" class="note"></div>
    <textarea id="c-text" spellcheck="false" placeholder="点「载入」读取当前声明原文；保存失败时这里的内容不会被清空或回滚。"></textarea>
    <div id="c-out"></div>
  </section>

  <!-- ── 5 · 审计 ───────────────────────────────────────── -->
  <section id="v-audit" class="view" hidden>
    <div class="bar">
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
      <span class="note">GET /api/audit?limit=（服务端上限 1000，返回顺序 oldest_first）。筛选在页面里做，只影响显示。</span>
    </div>
    <div id="a-summary" class="summary">尚未载入。</div>
    <div id="a-table"></div>
  </section>

  <!-- ── 6 · 查询台 ─────────────────────────────────────── -->
  <section id="v-query" class="view" hidden>
    <div class="panel">
      <h2>SPARQL（POST /api/sparql）</h2>
      <p class="note">查询服务只认 query token（租户由口令决定）。⭐ 服务**默认**回执是 JSON-LD 上下文子图；本页要表格，所以请求里显式带 <code>format:"rows"</code>（字节与旧的默认回执相同）。结果区**必须**同时给出 warnings / coverage / join_states / truncated —— 只给表格等于把「可能漏数据」藏起来。</p>
      <textarea id="q-text" spellcheck="false" placeholder="SELECT ?s WHERE { ?s a :&lt;类名&gt; }"></textarea>
      <div class="bar" style="margin-top:8px">
        <label>max_rows <input id="q-max" value="200" size="6"></label>
        <button id="q-go">执行</button>
        <span class="note">行数上限由 token 决定；页面最多渲染 200 行。</span>
      </div>
    </div>
    <div id="q-meta"></div>
    <div id="q-table"></div>
  </section>

  <!-- ── 7 · 目录浏览 ───────────────────────────────────── -->
  <section id="v-catalog" class="view" hidden>
    <div class="bar">
      <label>page <input id="k-page" value="1" size="4"></label>
      <label>page_size <input id="k-size" value="50" size="4"></label>
      <button id="k-load">载入</button>
      <span class="note">GET /api/catalog?page=&amp;page_size=（page_size ≤ 200）；点名字看 GET /api/describe?name=… 的详情。</span>
    </div>
    <div id="k-summary" class="summary">尚未载入。</div>
    <div id="k-table"></div>
    <div class="pager">
      <button id="k-prev">上一页</button>
      <span id="k-pos"></span>
      <button id="k-next">下一页</button>
    </div>
    <div id="k-detail" class="panel" hidden></div>
  </section>

  <!-- ── 8 · 构建包（REQ-159）────────────────────────────── -->
  <section id="v-bundles" class="view" hidden>
    <div class="bar">
      <button id="b-refresh">刷新</button>
      <span class="note">当前项目：</span><span id="b-project">—</span>
      <span class="note">（GET /api/projects）—— 本服务只服务启动时指定的那一个项目，所以这里只做显示。</span>
    </div>
    <div id="b-msg"></div>

    <div class="panel">
      <h2>1 · 上传并校验（POST …/bundles，application/octet-stream）</h2>
      <p class="note">包是构建端产出的 .ontokbundle 文件；页面只把**文件字节原样** POST 上去（⛔ 不套 JSON），服务端校验 sha256 与组件清单后回 state / bundle_id。</p>
      <div class="bar">
        <label>选包 <input id="b-file" type="file" accept=".ontokbundle"></label>
        <button id="b-upload">上传并校验</button>
      </div>
      <div id="b-state" class="summary">还没有上传。</div>
      <div id="b-components"></div>
    </div>

    <div id="b-dsn" class="panel" hidden>
      <h2>2 · 目标库连接（requires_dsn = true 时才需要）</h2>
      <p class="note">DSN 由本页按 postgresql 形式拼（user / password 走 encodeURIComponent）；口令**只在这一次 PUT 里出现**，保存成功后只清空口令框：不回显、不记录、不进 sessionStorage。</p>
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
        <span class="note">PUT …/database 的回执只有 database_id / host / database / user（⛔ 响应里没有 dsn，本页也不显示）；测试打的是服务端**已保存**的那份连接。</span>
      </div>
      <div id="b-dsn-out"></div>
    </div>

    <div class="panel">
      <h2>3 · 解析入库（POST …/bundles/&lt;id&gt;/import）</h2>
      <div class="bar">
        <button id="b-import" disabled>解析入库</button>
        <span class="note">条件没满足时按钮是灰的：先上传拿到 bundle_id；requires_dsn 的包还要 DSN 保存 + 连通测试通过。</span>
      </div>
      <div id="b-import-out"></div>
    </div>

    <div class="panel">
      <h2>4 · 待确认清单（GET /api/review?proposal=p1）</h2>
      <p class="note">本 tab 只做**汇总与门禁**：逐条裁决仍在「评审」tab 做（⛔ 这里不调 /api/decide —— 两套裁决入口等于两套口径）。待确认条数取服务端的 `progress.pending`（= total − signed，两轨都定了才算签完）、阻断条数取 `progress.vetoed`；只有**待确认 0 条**时「冻结并切流量」才可点。</p>
      <div class="bar"><button id="b-checklist-refresh">刷新清单</button></div>
      <div id="b-checklist" class="summary">尚未载入。</div>
    </div>

    <div class="panel">
      <h2>5 · 生效（冻结并切流量）· key</h2>
      <p class="note">激活 = 把这一版冻结、并让查询服务切过去；不可撤销。key 只在生成它的那次回执里出现**一次**，本页只放在内存里展示，关掉页面就没了。</p>
      <div class="bar">
        <button id="b-activate" class="danger" disabled>冻结并切流量…</button>
        <button id="b-rotate" class="danger">换 key…</button>
      </div>
      <div id="b-key" hidden>
        <div class="note">新 key（只显示这一次；离开页面就再也看不到）：</div>
        <div id="b-key-val" class="mono"></div>
        <div class="bar"><button id="b-key-copy">复制 key</button></div>
      </div>
    </div>
  </section>
</main>

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
var QUERY_KEY = "ontok.console.query";
var PAGE_SIZE_REVIEW = 40;
var UI_MAX_ROWS = 200;

var S = {
  at: "", qt: "", connected: false, tab: "projects",
  projects: null, review: null, reviewProposal: "", reviewPage: 1,
  declKind: "exclusions", audit: null, query: null,
  catalog: null, catalogPage: 1,
  // 构建包（REQ-159）：bundle = 上传回执 + 入库后补的 version；
  // pendingCount = 待确认条数（来源：/api/review 的 progress.pending，缺 progress 时按条目估算）
  bundle: null, needDsn: false, dsnOk: false, checklist: null, pendingCount: null
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
    "确认服务还在跑（端口 / 进程）；本页只允许同源 /api/*（CSP connect-src 'self'）——" +
    "查询台与目录浏览需要控制面与查询服务在同一个源后面（反向代理按路径分流）。");
}
function authHeader(useQuery) {
  var t = useQuery ? (S.qt || S.at) : S.at;
  return t ? { "Authorization": "Bearer " + t } : {};
}
// 查询端点「没接入」时的**固定文案**（404 与 403 共用）。
// 为什么两个状态说同一句话：直连 :8899 时，只填管理口令 ⇒ 404（console 没有这个路径）；
// 两个口令都填 ⇒ 403（console 认得 query token，但不接受它调管理端点）。两者都不是
// 「口令填错」，而是「查询端点不在此服务」——把用户引去改口令是死循环（L-1）。
var QUERY_WIRING = "查询服务未接入：这三个端点属于 query serve（另一进程）。" +
  "请把 query serve 挂到同源 /api/sparql、/api/catalog、/api/describe 下" +
  "（本机可用 tests/fixtures/req125/dev_proxy.ps1，部署指南见 BA/runbook/ontology-e2e.md §7.3）";
// 构建包 tab 的端点（REQ-159）可能还没上线：这类请求带 `notReady`，404 说同一句话。
// 与 QUERY_WIRING 同一条教训：**把「服务端没有这个路径」说成「口令不对」是死循环**。
var NOT_READY_MSG = "服务端未就绪：该端点还不存在（REQ-159 服务端在并行开发中）";
var NOT_READY_HINT = "这不是口令问题：页面**不会**清空口令、也不会退回登录态。" +
  "等服务端把这一组端点上线后重试；在那之前本页其余七个视图照常可用。";
function request(path, opts) {
  opts = opts || {};
  var headers = authHeader(!!opts.query);
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
      } else if (opts.query && (r.status === 404 || r.status === 403)) {
        // ⛔ **不清任何口令**：L-1 拓扑下两个口令都填了也还是 403，清掉就再也填不回来。
        err.code = "QUERY_NOT_WIRED";
        err.message = QUERY_WIRING;
        err.hint = "这不是口令问题：管理口令仍然有效，页面不会清空任何口令。" +
          "（用 query token 直连控制面时，控制面会以 403 拒绝「query token 调管理端点」——同上处理。）";
      } else if (opts.query && r.status === 401) {
        // 查询服务真的在、但缺 / 错 query token：只提示，**不动管理口令**。
        err.code = "QUERY_UNAUTHORIZED";
        err.message = "查询服务拒绝了这张口令（401）：查询台 / 目录浏览要的是 query token（含租户）。";
        err.hint = "在页面顶部第二个输入框填入 query token 后重试；⛔ 不会清掉你已经填的管理口令。";
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
function post(path, body, useQuery) {
  return json(path, { method: "POST", json: true, body: JSON.stringify(body), query: !!useQuery });
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
    if (S.qt) sessionStorage.setItem(QUERY_KEY, S.qt); else sessionStorage.removeItem(QUERY_KEY);
  } catch (e) { /* 隐私模式下可能不可用：内存里的口令仍然有效 */ }
}
function forget() {
  try { sessionStorage.removeItem(ADMIN_KEY); sessionStorage.removeItem(QUERY_KEY); } catch (e) {}
}
function logout(reason) {
  S.at = ""; S.qt = ""; S.connected = false;
  forget();
  $("login").hidden = false;
  $("token").value = "";
  $("qtoken").value = "";
  txt($("who"), "未连接");
  setMsg(reason || "已退回登录态。", "bad");
}
function enter(fromStore) {
  var at = fromStore ? S.at : $("token").value.trim();
  var qt = fromStore ? S.qt : $("qtoken").value.trim();
  if (!at) { setMsg("先填管理口令（admin token）。", "bad"); return; }
  S.at = at; S.qt = qt; S.connected = true;
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
    S.qt = sessionStorage.getItem(QUERY_KEY) || "";
  } catch (e) { S.at = ""; S.qt = ""; }
  if (S.at) { $("token").value = S.at; $("qtoken").value = S.qt; enter(true); }
  else setMsg("未连接：填管理口令后点「载入」。网友导航带不了 Authorization 头，所以页面壳先给你登录框。", "");
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
  sum.appendChild(el("b", null, d.project));
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
// 2 · 评审
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
  hd.appendChild(el("span", "target", it.target));
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
// 3 · 导出
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
  post("api/r2rml", body).then(function (r) {
    clear(out);
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
// 4 · 声明编辑
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
// 5 · 审计
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
    return [e.ts || "—", e.token || "—", e.action || "—", e.target || "—", e.result || "—", e.elapsed_ms];
  }), "还没有审计行（写操作与鉴权失败才会记）。"));
}

// ══════════════════════════════════════════════════════════
// 6 · 查询台 —— 四件元信息**一处**产出（AC-008）
// ══════════════════════════════════════════════════════════
// 纯函数：回执 JSON → 要显示的文案数组（{cls, text}）。
// 这不碰 DOM，所以 tests/fixtures/req125/page_probe.js 能直接用真实回执跑它。
function metaLines(r) {
  var out = [];
  if (!r) { out.push({ cls: "bad", text: "没有回执。" }); return out; }
  var warnings = r.warnings || [];
  if (warnings.length) {
    warnings.forEach(function (w) {
      out.push({ cls: "warn", text: "⚠ 可能漏数据（warnings）：" + w });
    });
  } else {
    out.push({ cls: "", text: "warnings：0 条（服务端没说可能漏数据）。" });
  }
  var coverage = r.coverage || [];
  coverage.forEach(function (c) {
    if (c && c.coverage === "main_only") {
      out.push({ cls: "warn", text: "⚠ 覆盖面 main_only：表 " + c.table + " —— 子账号产生的数据不在结果内，金额 / 条数可能偏小。" });
    }
  });
  var joins = r.join_states || [];
  var unused = joins.filter(function (j) { return j && j.used === false; });
  if (unused.length) {
    out.push({
      cls: "warn",
      text: "⚠ 涉及但未用的边（join_states[].used=false）共 " + unused.length + " 条：" +
        unused.map(function (j) { return j.from + " → " + j.to + "（" + j.predicate + "）"; }).join("；") +
        " —— 这些边在本次查询里**没有走到**（范围是「涉及的表之间的全部边」，不是实走边）。"
    });
  } else {
    out.push({ cls: "", text: "join_states[].used=false：0 条（本次查询涉及的边上，没有「涉及但未用」的）。" });
  }
  out.push({
    cls: r.truncated ? "bad" : "",
    text: "truncated：" + (r.truncated ? "是 —— 结果被截断（row_count=" + r.row_count + "）" : "否（row_count=" + (r.row_count || 0) + "）")
  });
  if (r.limits) {
    out.push({ cls: "", text: "limits：max_rows=" + r.limits.max_rows + " · token 上限=" + r.limits.token_max_rows + " · timeout_ms=" + r.limits.timeout_ms });
  }
  return out;
}
function renderQueryMeta(r) {
  var box = $("q-meta");
  clear(box);
  var warnBox = el("div", "panel");
  warnBox.appendChild(el("h2", null, "查询元信息（必须看：可能漏数据 / 覆盖面 / 未用的边 / 是否截断）"));
  var ul = el("ul", "warns");
  metaLines(r).forEach(function (l) {
    var li = el("li", l.cls || "", l.text);
    ul.appendChild(li);
  });
  warnBox.appendChild(ul);

  warnBox.appendChild(el("div", "note", "coverage："));
  warnBox.appendChild(makeTable(["表", "覆盖面"], (r.coverage || []).map(function (c) {
    return [c.table, badge(c.coverage, c.coverage === "main_only" ? "warn" : "ok")];
  }), "coverage 为空（查询没落到任何表？）。"));

  warnBox.appendChild(el("div", "note", "join_states（范围：" + (r.join_states_scope || "—") + "）："));
  warnBox.appendChild(makeTable(["边", "谓词", "状态", "本次用到", "证据"], (r.join_states || []).map(function (j) {
    return [
      j.from + " → " + j.to, j.predicate,
      badge(j.state, j.state === "true" ? "ok" : (j.state === "false" ? "bad" : "warn")),
      j.used ? badge("used", "ok") : badge("未使用", "warn"),
      j.evidence || "—"
    ];
  }), "join_states 为空（涉及的表之间没有映射边）。"));
  if (r.join_states_scope_note) warnBox.appendChild(el("div", "note", "范围说明：" + r.join_states_scope_note));
  warnBox.appendChild(el("div", "note", "tables：" + ((r.tables || []).join("、") || "—")));
  if (r.tenant_applied && r.tenant_applied.length) {
    warnBox.appendChild(el("div", "note", "tenant_applied：" + r.tenant_applied.map(function (t) {
      return JSON.stringify(t);
    }).join(" · ")));
  }
  if (r.public_dimensions && r.public_dimensions.length) {
    warnBox.appendChild(el("div", "note", "public_dimensions：" + r.public_dimensions.map(function (t) {
      return JSON.stringify(t);
    }).join(" · ")));
  }
  box.appendChild(warnBox);
}
function runQuery() {
  var q = $("q-text").value;
  if (!q.trim()) { setMsg("写一条 SPARQL 再执行。", "bad"); return; }
  var max = parseInt($("q-max").value, 10);
  // ⭐ REQ-142：查询服务的**默认**回执已翻转为 JSON-LD 上下文子图（@context/@graph/…）；
  // 工作台的「结果」区渲染的是行回执（columns/rows/row_count + 警告面板），所以这里
  // **显式**要 rows。⛔ 不写 format 会让页面拿到图、表格空转。
  var body = { query: q, format: "rows" };
  if (max > 0) body.max_rows = max;
  post("api/sparql", body, true).then(function (r) {
    S.query = r;
    renderQueryMeta(r);
    renderQueryTable(r);
    setMsg("查询完成：" + r.row_count + " 行" + (r.truncated ? "（已截断）" : ""), r.truncated ? "" : "ok");
  }).catch(function (e) {
    S.query = null;
    showErr($("q-meta"), e);
    clear($("q-table"));
    setMsg("查询失败：" + e.message, "bad");
  });
}
function renderQueryTable(r) {
  var box = $("q-table");
  clear(box);
  var cols = r.columns || [];
  var rows = (r.rows || []).slice(0, UI_MAX_ROWS);
  var panel = el("div", "panel");
  panel.appendChild(el("h2", null, "结果（" + (r.row_count || 0) + " 行" + (r.truncated ? " · 已截断" : "") + "）"));
  if ((r.rows || []).length > UI_MAX_ROWS) {
    panel.appendChild(el("div", "note", "只显示前 " + UI_MAX_ROWS + " 行（共 " + (r.rows || []).length + " 行）；要更多请缩小查询或加 LIMIT。"));
  }
  panel.appendChild(makeTable(cols, rows.map(function (row) {
    return row.map(function (v) { return (v === null || v === undefined) ? "—" : v; });
  }), "0 行。"));
  box.appendChild(panel);
}

// ══════════════════════════════════════════════════════════
// 7 · 目录浏览
// ══════════════════════════════════════════════════════════
function loadCatalog(page) {
  var p = page || parseInt($("k-page").value, 10) || 1;
  var size = Math.max(1, Math.min(200, parseInt($("k-size").value, 10) || 50));
  $("k-page").value = String(p);
  $("k-size").value = String(size);
  return json("api/catalog?page=" + p + "&page_size=" + size, { query: true }).then(function (d) {
    S.catalog = d; S.catalogPage = p;
    renderCatalog();
    return d;
  }).catch(function (e) {
    S.catalog = null;
    showErr($("k-table"), e);
    txt($("k-summary"), "载入失败。");
    throw e;
  });
}
function renderCatalog() {
  var d = S.catalog, sum = $("k-summary");
  clear(sum);
  if (!d) { txt(sum, "尚未载入。"); return; }
  var c = d.counts || {};
  sum.appendChild(el("b", null, d.project + " · 第 " + d.version + " 版"));
  sum.appendChild(el("span", null, " · 类 " + c.classes + " · 属性 " + c.properties + " · 关系 " + c.relations +
    " · 不可查关系 " + c.excluded_relations + " · 没有映射的类 " + c.skipped_classes +
    " · 本页 " + (d.items || []).length + "/" + d.total_items + " 条"));
  var box = $("k-table");
  clear(box);
  var rows = (d.items || []).map(function (it) {
    var name = el("button", null, it.name);
    name.onclick = function () { describeName(it.name); };
    var detail;
    if (it.kind === "class") {
      detail = "表 " + it.table + " · 主键 " + it.primary_key + " · " + it.property_count + " 属性 / " + it.relation_count + " 关系" +
        (it.description ? (" · " + it.description) : "");
    } else if (it.kind === "property") {
      detail = "类 " + it.class + " · 列 " + it.column + " · " + it.datatype +
        (it.enum_values ? (" · 枚举值 " + it.enum_values) : " · 枚举值 —（注释里没写）");
    } else {
      detail = it.from + " → " + it.to + " · 列 " + it.column + " · 反向 " + it.inverse +
        " · 证据等级 " + it.evidence_level + (it.cardinality ? (" · 基数 " + it.cardinality) : "");
    }
    return [badge(it.kind, it.kind === "class" ? "acc" : (it.kind === "relation" ? "warn" : "")), name, detail];
  });
  box.appendChild(makeTable(["类型", "名字（点开看详情）", "概要"], rows, "这一页没有条目。"));
  if ((d.skipped_classes || []).length) {
    var p = el("div", "panel");
    p.appendChild(el("h2", null, "没有 TriplesMap 的类（不可查，但不是静默少了几张表）"));
    p.appendChild(makeTable(["表", "原因"], d.skipped_classes.map(function (s) { return [s.table, s.reason]; })));
    box.appendChild(p);
  }
  txt($("k-pos"), "第 " + d.page + " / " + d.total_pages + " 页（page_size=" + d.page_size + "）");
  $("k-prev").disabled = d.page <= 1;
  $("k-next").disabled = !d.has_more;
}
function describeName(name) {
  json("api/describe?name=" + encodeURIComponent(name), { query: true }).then(function (d) {
    var box = $("k-detail");
    clear(box);
    box.hidden = false;
    box.appendChild(el("h2", null, "详情：" + d.name + "（" + d.kind + "）"));
    if (d.kind === "class") {
      box.appendChild(kvList([
        ["源表", d.table], ["主键", d.primary_key], ["说明", d.description || "—"],
        ["租户口径", (d.tenant ? (d.tenant.mode + (d.tenant.column ? ("（列 " + d.tenant.column + "）") : "") +
          (d.tenant.coverage ? (" · 覆盖面 " + d.tenant.coverage) : "") + (d.tenant.reason ? (" · " + d.tenant.reason) : "")) : "—")]
      ]));
      box.appendChild(el("h3", null, "属性（" + (d.properties || []).length + "）"));
      box.appendChild(makeTable(["名字", "列", "类型", "可空", "枚举值", "样例", "说明"], (d.properties || []).map(function (p) {
        return [p.name, p.column, p.datatype, p.nullable === null || p.nullable === undefined ? "—" : yesNo(p.nullable),
          p.enum_values || "—", p.sample === null || p.sample === undefined ? "—" : p.sample, p.note || "—"];
      })));
      box.appendChild(el("h3", null, "关系（" + (d.relations || []).length + "）"));
      box.appendChild(makeTable(["名字", "反向", "边", "证据等级", "可查", "基数", "证据"], (d.relations || []).map(function (r) {
        return [r.name, r.inverse, r.from + " → " + r.to + " · 列 " + r.column,
          badge(r.evidence_level, r.evidence_level === "true" ? "ok" : (r.evidence_level === "false" ? "bad" : "warn")),
          r.queryable ? badge("可查", "ok") : badge("不可查", "bad"),
          r.cardinality || "—", r.evidence || "—"];
      })));
    } else {
      box.appendChild(kvList([
        ["类", d.class], ["名字", d.name], ["列", d.column], ["类型", d.datatype],
        ["枚举值", d.enum_values || "—（注释里没写）"],
        ["可空", d.nullable === null || d.nullable === undefined ? "—" : yesNo(d.nullable)],
        ["样例", d.sample === null || d.sample === undefined ? "—" : d.sample],
        ["说明", d.note || "—"]
      ]));
    }
  }).catch(function (e) {
    var box = $("k-detail");
    box.hidden = false;
    showErr(box, e);
  });
}

// ══════════════════════════════════════════════════════════
// 8 · 构建包（REQ-159）—— 上传 / 目标库 / 入库 / 待确认门禁 / 生效
// ══════════════════════════════════════════════════════════
// 这一组的端点在服务端是**并行开发**的：本 tab 的每次请求都带 notReady，
// 404 ⇒ 「服务端未就绪」并且**不登出**（见上面的 request 层）。
var LAST_KEY = null;   // ⭐ key 只在内存：⛔ 不进 sessionStorage、不回填任何输入框

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
    S.projects = d;
    txt($("b-project"), d.project || "—");
    clear($("b-msg"));
    loadChecklist().catch(function () {});
    return d;
  }).catch(function (e) {
    txt($("b-project"), "—");
    showErr($("b-msg"), e);
    throw e;
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
  box.appendChild(makeTable(["组件", "计数", "requires_dsn", "dialect"], ["ontology", "am", "db"].map(function (k) {
    var c = comps[k] || null;
    return [k, c ? countOf(c) : "—", c ? flagOf(c.requires_dsn) : "—", c ? (c.dialect || "—") : "—"];
  }), "组件清单为空。"));
  needDsn(res);
  renderBundleGates();
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
    if (r.version !== undefined) b.version = r.version;
    if (r.state) b.state = r.state;
    S.bundle = b;
    out.appendChild(okBox([
      "版本：" + (r.version === undefined ? "—" : ("第 " + r.version + " 版")),
      "state：" + (r.state || "—")
    ]));
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
    setMsg("已解析入库。", "ok");
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
      "已生效：state " + (r.state || "—") +
      " · 版本 " + (r.version === undefined ? "—" : r.version) +
      " · ontology_version " + (r.ontology_version === undefined ? "—" : r.ontology_version)
    ]));
    b.state = r.state || "active";
    S.bundle = b;
    if (r.key) showKey(r.key);   // 回执里带 key 就按「只显示一次」处理
    setMsg("已冻结并切流量。", "ok");
    renderBundleGates();
  }).catch(function (e) {
    showErr($("b-import-out"), e);
    setMsg("激活失败：" + e.message, "bad");
  });
}

// key 只进内存变量 + 一个只读展示块：⛔ 不写 sessionStorage、不回填输入框。
function showKey(k) {
  LAST_KEY = k || "";
  $("b-key").hidden = !LAST_KEY;
  txt($("b-key-val"), LAST_KEY || "（这一次回执里没有 key）");
  return LAST_KEY;
}
function copyKey() {
  if (!LAST_KEY) { setMsg("还没有可复制的 key（先在「换 key」里生成）。", "bad"); return; }
  if (navigator && navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(LAST_KEY).then(function () {
      setMsg("key 已复制到剪贴板（仍然只在内存里）。", "ok");
    }).catch(function () {
      setMsg("复制被浏览器拒绝：请手动选中 key 那一行复制；离开页面就没了。", "bad");
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
      ["后果", "旧 key **立刻失效**：还在用旧 key 的调用方会 401；新 key 只在这一次回执里出现，关掉页面就再也拿不到（只能再换一次）。"]
    ],
    ok: "换 key"
  }, function () {
    return bjson(bundlePath("/key/rotate"), "POST", {});
  }).then(function (r) {
    if (r === null) { setMsg("已取消：没有发出任何请求。", ""); return; }
    r = r || {};
    showKey(r.key);
    var out = $("b-import-out");
    if (r.key_digest) out.appendChild(el("div", "note", "key_digest：" + r.key_digest + "（摘要可以留档；key 本身只显示一次）"));
    setMsg("已换 key：新 key 只显示这一次。", "ok");
  }).catch(function (e) {
    showErr($("b-import-out"), e);
    setMsg("换 key 失败：" + e.message, "bad");
  });
}

// ══════════════════════════════════════════════════════════
// tab 切换（无 URL 状态：地址栏永远不带口令）
// ══════════════════════════════════════════════════════════
var TABS = ["projects", "review", "export", "config", "audit", "query", "catalog", "bundles"];
function switchTab(name) {
  S.tab = name;
  TABS.forEach(function (t) {
    $("v-" + t).hidden = (t !== name);
    var btn = $("tab-" + t);
    btn.className = (t === name) ? "on" : "";
  });
  if (name === "review" && !S.review) loadReview().catch(function () {});
  if (name === "audit" && !S.audit) loadAudit().catch(function () {});
  if (name === "export" && !S.projects) loadProjects().catch(function () {});
  if (name === "catalog" && !S.catalog) loadCatalog(1).catch(function () {});
  if (name === "bundles") loadBundles().catch(function () {});
}

// ══════════════════════════════════════════════════════════
// 绑定 + 启动
// ══════════════════════════════════════════════════════════
$("go").onclick = function () { enter(false); };
$("token").addEventListener("keydown", function (e) { if (e.key === "Enter") enter(false); });
$("qtoken").addEventListener("keydown", function (e) { if (e.key === "Enter") enter(false); });
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
$("q-go").onclick = runQuery;
$("k-load").onclick = function () { loadCatalog(parseInt($("k-page").value, 10) || 1).catch(function () {}); };
$("k-prev").onclick = function () { loadCatalog(Math.max(1, S.catalogPage - 1)).catch(function () {}); };
$("k-next").onclick = function () { if (S.catalog && S.catalog.has_more) loadCatalog(S.catalogPage + 1).catch(function () {}); };
$("b-refresh").onclick = function () { loadBundles().catch(function () {}); };
$("b-upload").onclick = uploadBundle;
$("b-dsn-save").onclick = saveDsn;
$("b-dsn-test").onclick = testDsn;
$("b-import").onclick = importBundle;
$("b-checklist-refresh").onclick = function () { loadChecklist().catch(function () {}); };
$("b-activate").onclick = activateBundle;
$("b-rotate").onclick = rotateKey;
$("b-key-copy").onclick = copyKey;

restore();

// ⭐ 纯函数 / 可注入点的唯一出口：`tests/fixtures/req125/page_probe.js` 从这里跑
// metaLines（AC-008）、guarded（取消不发请求）、el / makeTable（XSS）、remember（sessionStorage）。
if (typeof window !== "undefined") {
  window.WBCORE = {
    metaLines: metaLines,
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

    /// 八个视图都在（AC-004）。
    #[test]
    fn the_shell_has_all_views() {
        for needle in [
            "项目与版本",
            "评审",
            "导出",
            "声明编辑",
            "审计",
            "查询台",
            "目录浏览",
            "构建包",
        ] {
            assert!(CONSOLE_HTML.contains(needle), "页面缺视图 {needle:?}");
        }
        for tab in [
            "projects", "review", "export", "config", "audit", "query", "catalog", "bundles",
        ] {
            assert!(
                CONSOLE_HTML.contains(&format!("id=\"v-{tab}\"")),
                "缺视图容器 v-{tab}"
            );
            assert!(
                CONSOLE_HTML.contains(&format!("id=\"tab-{tab}\"")),
                "缺 tab 按钮 tab-{tab}"
            );
        }
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
            "key/rotate",
            // 上传必须是原始字节，不能走会 JSON 化的 post()
            "application/octet-stream",
            "ctype",
            // 目标库面板：口令框只支持新密码、不回填
            "b-dsn-pass",
            "new-password",
            // 门禁与一次性 key
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
        ] {
            assert!(
                CONSOLE_HTML.contains(needle),
                "构建包视图缺少 {needle:?}（REQ-159 的契约）"
            );
        }
        for fun in [
            "loadBundles()",
            "uploadBundle()",
            "showBundle(",
            "needDsn(",
            "testDsn()",
            "saveDsn()",
            "importBundle()",
            "loadChecklist()",
            "activateBundle()",
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
        let bundles_at = CONSOLE_HTML.find("// 8 · 构建包").expect("缺构建包 JS 段");
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

    /// 查询台的四段元信息由同一处产出（AC-008）。
    #[test]
    fn the_query_view_declares_all_four_meta_fields() {
        for needle in ["warnings", "coverage", "join_states", "used === false", "truncated"] {
            assert!(
                CONSOLE_HTML.contains(needle),
                "查询台缺少 {needle:?} 的处理（AC-008：只显示表格 = 不合格）"
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
}
