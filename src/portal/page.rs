//! REQ-126 · **客户门户的页面壳** —— 自包含单文件、零外部依赖、**壳内零数据**。
//!
//! # 这个常量里**没有**什么（AC-005 的正面判据）
//!
//! 项目名、版本号、业务对象名、问题标题、口径说明 —— 一个都不在。壳只回答
//! 「门在哪里、问了之后长什么样」：登录框、卡片容器、结果容器、错误容器。
//! `GET /` 的响应体逐字节就是这个常量（服务端**不注入任何东西**），
//! 于是「壳里有没有数据」用 `grep` 就能判，不需要跑浏览器。
//!
//! 问题列表走 `GET /api/questions`（带口令），执行走 `POST /q/<id>`。
//!
//! # 为什么是单文件 + 内联脚本/样式
//!
//! 客户拿到的是一个地址，不是一个构建产物：⛔ 没有框架、⛔ 没有 CDN、⛔ 没有构建步骤。
//! 代价写在 CSP 里（[`CSP`]）：`script-src` / `style-src` 必须带 `'unsafe-inline'`，
//! 因为脚本与样式就在这份 HTML 里。这不是「图省事」——**拿掉它页面就整段不执行**。
//!
//! **代价（必须承认，不许写成「已防护」）**：`'unsafe-inline'` 让浏览器不再拦截
//! 内联脚本注入 ⇒ CSP 对「数据里带 `<script>`」这条路**帮不上忙**，页面自己必须兜住。
//! 本页的兜法是**只走 `textContent`**：所有服务端字符串（对象名 / 说明 / 告警 /
//! 错误信息 / 单元格）都经 `el()` / `txt()` 写入；页面里
//! `innerHTML` / `outerHTML` / `insertAdjacentHTML` / `document.write` / `eval(`
//! **各 0 处**（判据：`tests/test_req126_portal.rs`）。
//!
//! # 口令只进 sessionStorage
//!
//! 与 REQ-125 的登录态写法同源：不进地址栏（不读 `location.search`）、不写长期存储、
//! 不写 cookie；关掉标签页就没了。⛔ 页面里没有任何口令值（连假值都不放）。
//!
//! # 已知边界
//!
//! - 没有国际化 / 主题切换开关（跟随系统深浅色）；
//! - 没有分页：服务端按口令的 `max_rows` 截断，页面把「只显示前 N 行」写出来；
//! - 结果表不做列宽 / 排序 / 导出 —— 客户要的是「看一个数」，不是报表工具。

use axum::http::{HeaderName, HeaderValue};

/// 页面壳（**零数据**：任何项目名 / 版本号 / 对象名 / 问题标题都不在这里）。
///
/// ⛔ 改这个常量时守住五件事：
/// 1. 不出现 `src=` / `href=` 的外部资源、不出现 `@import`；
/// 2. 不出现任何真实数据（项目名、版本号、对象名、表名、口令、问题标题）；
/// 3. 用户 / 服务端字符串一律 `textContent`（`el` / `txt`），**没有** `innerHTML`；
/// 4. 口令只进 `sessionStorage`；
/// 5. 只有**一个** `fetch(` 调用点（请求层），不自己造第二套。
pub const PORTAL_HTML: &str = r####"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>客户数据门户</title>
<style>
:root{--bg:#141416;--fg:#f2f2f7;--mut:#9b9ba1;--line:#3c3c40;--card:#1c1c1f;--acc:#4d9bff;--ok:#3fbf6a;--bad:#ff6b5e;--warn:#ffb340;--head:#232327}
@media (prefers-color-scheme: light){
  :root{--bg:#fbfbfd;--fg:#1d1d1f;--mut:#6e6e73;--line:#d2d2d7;--card:#fff;--acc:#0a6cff;--ok:#0a8f3c;--bad:#c2261c;--warn:#b26a00;--head:#f2f2f5}
}
*{box-sizing:border-box}
body{margin:0;font:14px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Hiragino Sans GB","Microsoft YaHei",sans-serif;background:var(--bg);color:var(--fg)}
header{position:sticky;top:0;z-index:8;background:var(--head);border-bottom:1px solid var(--line);padding:10px 16px;display:flex;gap:10px;align-items:center;flex-wrap:wrap}
h1{font-size:15px;margin:0;font-weight:600;white-space:nowrap}
h2{font-size:16px;margin:0 0 6px}
h3{font-size:13px;margin:0 0 6px}
#who{color:var(--mut);font-size:13px}
#msg{font-size:13px;flex:1;min-width:200px;color:var(--mut)}
#msg.bad{color:var(--bad);font-weight:600}
#msg.ok{color:var(--ok)}
input{font:inherit;color:var(--fg);background:var(--card);border:1px solid var(--line);border-radius:6px;padding:5px 9px}
input[type=password]{width:210px}
button{font:inherit;padding:5px 12px;border:1px solid var(--line);border-radius:6px;background:var(--card);color:var(--fg);cursor:pointer}
button:hover{border-color:var(--acc);color:var(--acc)}
button:disabled{opacity:.45;cursor:default}
main{padding:16px;max-width:1080px;margin:0 auto 80px}
.note{color:var(--mut);font-size:12px;margin:4px 0 0}
.lead{color:var(--mut);font-size:13px;margin:0 0 14px}
.cards{display:grid;grid-template-columns:repeat(auto-fill,minmax(260px,1fr));gap:10px}
.card{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:12px 14px;text-align:left}
.card h3{margin:0;font-size:14px;color:var(--fg)}
.card .note{margin-top:6px}
.card button{margin-top:10px}
#answer{margin-top:20px}
.panel{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:12px 14px;margin:12px 0}
.warnbox{background:var(--card);border:2px solid var(--warn);border-radius:10px;padding:12px 14px;margin:12px 0}
.warnbox h3{color:var(--warn)}
ul.warns{margin:4px 0 0;padding-left:20px}
ul.warns li{margin:5px 0;color:var(--warn);font-weight:600}
.summary{color:var(--mut);font-size:13px;margin:6px 0 10px}
.summary b{color:var(--fg)}
table{width:100%;border-collapse:collapse;font-size:13px}
th,td{text-align:left;padding:5px 9px;border-bottom:1px solid var(--line);vertical-align:top;word-break:break-word}
th{font-weight:600;font-size:12px;color:var(--mut);white-space:nowrap}
td.num{font-variant-numeric:tabular-nums;text-align:right;white-space:nowrap}
.badge{display:inline-block;font-size:12px;padding:0 8px;border-radius:999px;border:1px solid var(--line);color:var(--mut);white-space:nowrap}
.badge.warn{color:var(--warn);border-color:var(--warn)}
.badge.ok{color:var(--ok);border-color:var(--ok)}
.err{background:var(--card);border:1px solid var(--bad);border-radius:10px;padding:12px 14px;margin:12px 0}
.err .code{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:12px;color:var(--bad);font-weight:600}
.err .msg{margin-top:6px}
.err .hint{margin-top:6px;color:var(--mut);font-size:12px}
.empty{color:var(--mut);text-align:center;padding:26px 0}
[hidden]{display:none !important}
</style>
</head>
<body>
<header>
  <h1>客户数据门户</h1>
  <span id="who">未登录</span>
  <span id="login">
    <input id="token" type="password" placeholder="访问口令" autocomplete="off">
    <button id="go">进入</button>
  </span>
  <button id="out" hidden>退出</button>
  <span id="msg">口令只留在本标签页的 sessionStorage 里：不进地址栏、不写长期存储。</span>
</header>

<main>
  <p class="lead">这里是你的数据：选择一个问题查看结果。数据范围由访问口令决定 —— 不需要、也不能指定查谁。</p>
  <div id="cards" class="cards"></div>
  <section id="answer" hidden>
    <h2 id="a-title"></h2>
    <p id="a-note" class="note"></p>
    <div id="a-warns"></div>
    <div class="panel">
      <h3>这份结果涉及的业务对象</h3>
      <div id="a-objects"></div>
    </div>
    <div id="a-meta" class="summary"></div>
    <div id="a-table"></div>
  </section>
  <div id="err"></div>
</main>

<script>
(function () {
  "use strict";

  var KEY = "ontok.portal.storage.1";

  function $(id) { return document.getElementById(id); }

  function txt(node, s) {
    node.textContent = (s === undefined || s === null) ? "" : String(s);
  }

  function clear(node) { node.textContent = ""; }

  function el(tag, cls, text) {
    var n = document.createElement(tag);
    if (cls) { n.className = cls; }
    if (text !== undefined) { txt(n, text); }
    return n;
  }

  function setMsg(s, kind) {
    var m = $("msg");
    txt(m, s);
    m.className = kind || "";
  }

  function getSecret() {
    try { return sessionStorage.getItem(KEY) || ""; } catch (e) { return ""; }
  }

  function setSecret(v) {
    try {
      if (v) { sessionStorage.setItem(KEY, v); } else { sessionStorage.removeItem(KEY); }
    } catch (e) { /* 隐私模式下 sessionStorage 可能不可用：退化成「本次会话不记住」 */ }
  }

  function showLogin(show) {
    $("login").hidden = !show;
    $("out").hidden = show;
    txt($("who"), show ? "未登录" : "已登录（本标签页）");
  }

  // ── 请求层：全页唯一的 fetch 调用点（同源相对路径）──────────
  function request(path, method, body) {
    var opt = { method: method || "GET", headers: {} };
    var secret = getSecret();
    if (secret) { opt.headers["Authorization"] = "Bearer " + secret; }
    if (body !== undefined && body !== null) {
      opt.headers["Content-Type"] = "application/json";
      opt.body = JSON.stringify(body);
    }
    return fetch(path, opt).then(function (res) {
      return res.text().then(function (s) {
        var v = null;
        try { v = s ? JSON.parse(s) : null; } catch (e) { v = null; }
        if (!res.ok) {
          var err = new Error("request failed");
          err.status = res.status;
          err.body = v;
          throw err;
        }
        return v;
      });
    });
  }

  function clearError() { clear($("err")); }

  function showError(e) {
    var box = $("err");
    clear(box);
    var card = el("div", "err");
    card.appendChild(el("div", "code", (e && e.body && e.body.code) ? e.body.code : "ERROR"));
    card.appendChild(el("div", "msg", (e && e.body && e.body.message) ? e.body.message : "这次没有取到数据。"));
    var hint = (e && e.body && e.body.hint) ? e.body.hint : "";
    if (!hint && e && e.status === 0) { hint = "网络中断了？刷新页面再试。"; }
    if (hint) { card.appendChild(el("div", "hint", hint)); }
    box.appendChild(card);
  }

  function onUnauthorized() {
    setSecret("");
    showLogin(true);
    clear($("cards"));
    $("answer").hidden = true;
    setMsg("口令无效或已失效：请重新输入访问口令。", "bad");
  }

  // ── 问题列表 ────────────────────────────────────────────────
  function renderCards(list) {
    var host = $("cards");
    clear(host);
    if (!list || list.length === 0) {
      host.appendChild(el("div", "empty", "目前没有可查看的问题。"));
      return;
    }
    for (var i = 0; i < list.length; i++) {
      (function (q) {
        var card = el("article", "card");
        card.appendChild(el("h3", null, q.title));
        if (q.note) { card.appendChild(el("p", "note", q.note)); }
        var btn = el("button", null, "查看");
        btn.addEventListener("click", function () { ask(q.id, btn); });
        card.appendChild(btn);
        host.appendChild(card);
      })(list[i]);
    }
  }

  function loadQuestions() {
    if (!getSecret()) { showLogin(true); return; }
    clearError();
    setMsg("正在载入问题列表…", "");
    request("/api/questions", "GET").then(function (v) {
      showLogin(false);
      renderCards(v ? v.questions : []);
      setMsg("选一个问题点「查看」。", "ok");
    }).catch(function (e) {
      if (e.status === 401) { onUnauthorized(); return; }
      showLogin(false);
      showError(e);
      setMsg("问题列表没能载入。", "bad");
    });
  }

  // ── 执行一个问题 ────────────────────────────────────────────
  function ask(id, btn) {
    clearError();
    $("answer").hidden = true;
    if (btn) { btn.disabled = true; }
    setMsg("正在取数…", "");
    request("/q/" + encodeURIComponent(id), "POST", {}).then(function (v) {
      renderAnswer(v);
      setMsg("取数完成。", "ok");
    }).catch(function (e) {
      if (e.status === 401) { onUnauthorized(); return; }
      showError(e);
      setMsg("这次没有取到数据。", "bad");
    }).then(function () {
      if (btn) { btn.disabled = false; }
    });
  }

  function renderAnswer(v) {
    if (!v) { return; }
    var q = v.question || {};
    txt($("a-title"), q.title);
    var note = $("a-note");
    if (q.note) { txt(note, q.note); note.hidden = false; } else { clear(note); note.hidden = true; }

    // ① 覆盖警告 / 截断 —— 显著区（页面里唯一有 2px 边框的块）
    var warnHost = $("a-warns");
    clear(warnHost);
    var warns = (v.warnings && v.warnings.length) ? v.warnings : [];
    if (warns.length > 0) {
      var box = el("div", "warnbox");
      box.appendChild(el("h3", null, "请注意：这个数字可能不完整"));
      var ul = el("ul", "warns");
      for (var i = 0; i < warns.length; i++) { ul.appendChild(el("li", null, warns[i])); }
      box.appendChild(ul);
      warnHost.appendChild(box);
    }

    // ② 数据说明：涉及哪些业务对象 + 覆盖范围
    var objHost = $("a-objects");
    clear(objHost);
    var objects = v.objects || [];
    if (objects.length === 0) {
      objHost.appendChild(el("div", "note", "（服务端没有报告参与的对象）"));
    } else {
      var t = el("table");
      var thead = el("thead");
      var hr = el("tr");
      hr.appendChild(el("th", null, "业务对象"));
      hr.appendChild(el("th", null, "覆盖范围"));
      thead.appendChild(hr);
      t.appendChild(thead);
      var tb = el("tbody");
      for (var k = 0; k < objects.length; k++) {
        var row = el("tr");
        row.appendChild(el("td", null, objects[k].name));
        var badge = el("span", objects[k].coverage === "main_only" ? "badge warn" : "badge ok",
                       objects[k].coverage_label);
        var cell = el("td");
        cell.appendChild(badge);
        row.appendChild(cell);
        tb.appendChild(row);
      }
      t.appendChild(tb);
      objHost.appendChild(t);
    }

    // ③ 行数 / 截断 / 耗时 / 上限
    var meta = $("a-meta");
    clear(meta);
    var lim = v.limits || {};
    meta.appendChild(el("span", null, "共 "));
    meta.appendChild(el("b", null, String(v.row_count || 0)));
    meta.appendChild(el("span", null, " 行 · 用时 " + String(v.elapsed_ms || 0) + " ms · 本次上限 " +
                                     String(lim.max_rows || 0) + " 行 · 超时 " + String(lim.timeout_ms || 0) + " ms"));
    if (v.truncated) {
      meta.appendChild(el("span", null, " —— 结果超过上限，只显示前 " +
                                       String(lim.max_rows || 0) + " 行，还有更多没有显示"));
    }

    // ④ 结果表
    var tableHost = $("a-table");
    clear(tableHost);
    var cols = v.columns || [];
    var rows = v.rows || [];
    if (rows.length === 0) {
      tableHost.appendChild(el("div", "empty", "没有数据。这可能是正常的：你名下没有这类记录。"));
    } else {
      var tbl = el("table");
      var th = el("thead");
      var tr = el("tr");
      for (var c = 0; c < cols.length; c++) { tr.appendChild(el("th", null, cols[c])); }
      th.appendChild(tr);
      tbl.appendChild(th);
      var body = el("tbody");
      for (var r = 0; r < rows.length; r++) {
        var line = el("tr");
        for (var c2 = 0; c2 < cols.length; c2++) {
          var raw = rows[r][c2];
          var val = (raw === null || raw === undefined) ? "—" : String(raw);
          var td = el("td", (raw === null || raw === undefined) ? null : "num", val);
          line.appendChild(td);
        }
        body.appendChild(line);
      }
      tbl.appendChild(body);
      tableHost.appendChild(tbl);
    }

    $("answer").hidden = false;
  }

  // ── 登录 / 退出 ─────────────────────────────────────────────
  $("go").addEventListener("click", function () {
    var v = $("token").value.trim();
    if (!v) { setMsg("先填访问口令。", "bad"); return; }
    setSecret(v);
    $("token").value = "";
    loadQuestions();
  });

  $("token").addEventListener("keydown", function (ev) {
    if (ev.key === "Enter") { $("go").click(); }
  });

  $("out").addEventListener("click", function () {
    setSecret("");
    clear($("cards"));
    clear($("err"));
    $("answer").hidden = true;
    showLogin(true);
    setMsg("已退出：口令已从本标签页清除。", "");
  });

  if (getSecret()) { loadQuestions(); } else { showLogin(true); }
})();
</script>
</body>
</html>
"####;

/// 门户页面的 CSP。
///
/// `default-src 'self'` 是下限；`connect-src 'self'` 把「页面被改成打第三方」
/// 变成一条会当场失败的请求（也解释了为什么门户必须自己提供 API —— 它不跨源）。
/// `frame-ancestors 'none'` 禁止被别的站点套框（门户里是客户数据）。
///
/// `'unsafe-inline'` 的代价与兜法见模块头（⛔ 那不是「已防护」）。
pub const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; \
base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// 页面 + API 响应必须带的三个头（AC-007）。
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

    #[test]
    fn the_shell_has_no_external_dependency() {
        for needle in [
            "<script src=",
            "<link ",
            "href=",
            "https://",
            "http://",
            "@import",
            "integrity=",
            "//cdn",
        ] {
            assert!(
                !PORTAL_HTML.contains(needle),
                "页面壳里出现了外部引用 {needle:?}（AC-007：零依赖）"
            );
        }
    }

    #[test]
    fn the_shell_never_uses_innerhtml_or_document_write() {
        for needle in [
            "innerHTML",
            "outerHTML",
            "insertAdjacentHTML",
            "document.write",
            "eval(",
        ] {
            assert!(
                !PORTAL_HTML.contains(needle),
                "页面壳里出现了 {needle:?} —— 用户数据只能走 textContent"
            );
        }
        // 本页只有 `txt()` / `clear()` 两个 `textContent` 写点 —— 所有服务端字符串
        // 都从它们走（这也正是「只走 textContent」的样子）。⛔ 别为了凑数再抄第三个写点。
        assert!(
            PORTAL_HTML.matches("textContent").count() >= 2,
            "页面必须有 textContent 写点（不是「零个写点所以全绿」）"
        );
        assert!(
            PORTAL_HTML.matches("txt(").count() >= 6,
            "服务端字符串应当都经 txt() 写入"
        );
    }

    #[test]
    fn the_shell_only_uses_session_storage_for_the_secret() {
        assert!(
            !PORTAL_HTML.contains("localStorage")
                && !PORTAL_HTML.contains("indexedDB")
                && !PORTAL_HTML.contains("document.cookie"),
            "口令只能存 sessionStorage"
        );
        assert!(PORTAL_HTML.contains("sessionStorage"), "页面必须用 sessionStorage");
        assert!(!PORTAL_HTML.contains("location.search"));
        assert!(!PORTAL_HTML.contains("location.hash"));
        assert!(!PORTAL_HTML.contains("token="), "口令不许出现在 URL 里");
        assert!(!PORTAL_HTML.contains("REPLACE-WITH"), "口令样例不许进页面");
    }

    /// 只有一个 `fetch(` 调用点：别的请求都走 `request`（结构上没有第二套请求层）。
    #[test]
    fn the_shell_has_exactly_one_fetch_call_site() {
        assert_eq!(
            PORTAL_HTML.matches("fetch(").count(),
            1,
            "页面只应有一个 fetch 调用点（request 层）"
        );
        assert!(!PORTAL_HTML.contains("XMLHttpRequest"), "不许自己造请求层");
        assert!(!PORTAL_HTML.contains("\"//"), "不许出现协议相对 URL");
    }

    /// 壳是**零数据**的（AC-005）：问题标题 / 对象名 / 项目名 / 版本提示都不许在。
    #[test]
    fn the_shell_carries_no_data() {
        for needle in [
            "anspire",
            "r2rml",
            "扣费",
            "余额",
            "订阅",
            "授权",
            "用量",
            "第 2 版",
            "deductions",
            "balances",
            "licenses",
            "usage_details",
            "customer_subscriptions",
            "package_templates",
            "demo",
        ] {
            assert!(
                !PORTAL_HTML.contains(needle),
                "页面壳里出现了数据 {needle:?}（AC-005：壳必须零数据）"
            );
        }
    }

    /// 只有四个端点被页面消费（⛔ 没有自由查询入口）。
    #[test]
    fn the_shell_only_talks_to_the_four_endpoints() {
        for endpoint in ["/api/questions", "/q/"] {
            assert!(PORTAL_HTML.contains(endpoint), "页面没有消费 {endpoint}");
        }
        for forbidden in ["/api/sparql", "/api/catalog", "/api/describe", "/api/apply"] {
            assert!(
                !PORTAL_HTML.contains(forbidden),
                "页面里出现了不该有的端点 {forbidden:?}（AC-002：没有自由查询）"
            );
        }
    }

    #[test]
    fn the_csp_keeps_requests_same_origin() {
        assert!(CSP.contains("default-src 'self'"), "{CSP}");
        assert!(CSP.contains("connect-src 'self'"), "{CSP}");
        assert!(CSP.contains("frame-ancestors 'none'"), "{CSP}");
        assert_eq!(SECURITY_HEADERS.len(), 3);
    }
}
