//! REQ-093 · 图视图页面 —— **自包含单文件，零外部依赖**。
//!
//! 沿用 `domain/html_view.rs` 的三条硬约束（风格照抄，数据换了）：
//!
//! 1. **零外部依赖**：不引 CDN、不引外部字体、不用构建工具。
//!    判读人可能**离线**打开；CSS/JS 全部内联，字体只用系统字体栈。
//! 2. **零建模术语**：页面本身也过 `scan_modeling_terms`
//!    （与 `cli.rs` 对 `--html` 的处置同一条），所以下面**不用**「证据」
//!    这个词 —— 它在守门词表里，一律改说「说明」。
//! 3. **关系按人的说法显示**：边上的字直接来自客户视图，
//!    不退回 `belongs_to` / `N:1` 这类建模记号。
//!
//! # 布局为什么手写
//!
//! 力导向的核心就是「斥力 + 弹簧 + 向心」三段，91 个节点 / 79 条边
//! 两两斥力是 8281 对 × 250 轮 ≈ 200 万次 —— 浏览器里毫秒级，
//! 换来的是**零字节依赖**。引一个图布局库要内联进这个文件，
//! 体积大出一个数量级，而规格 §6.4 也明说不需要图数据库。
//! 代价见「已知限制」：**没有 Barnes-Hut**，节点数上千会明显变卡。
//!
//! # ⚠️ 守门器连 **JS 属性名**一起扫
//!
//! 写这页时踩过一次：`$("#t").addEventListener("keydown", function (e) { e.key … })`
//! 里的 **`e.key`** 被判成守门词 `key` —— 页面源码因此**整份拒发**。
//! 分词按「非字母数字/下划线」切，所以 `e.key` 就是一个 `key` 词元。
//! 已改成 `e.code`。`tests/test_graph_server.rs::the_page_itself_has_no_modeling_terms`
//! 钉住这条（它当时确实红过一次，不是假想）。

/// 页面本体。**注意**：这里出现的每一个词都会被守门器扫过。
pub(crate) const INDEX_HTML: &str = r##"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>业务关系图</title>
<style>
:root{--bg:#fbfbfd;--fg:#1d1d1f;--mut:#6e6e73;--line:#c7c7cc;--acc:#0a6cff;--card:#fff}
*{box-sizing:border-box}
body{margin:0;font:14px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Hiragino Sans GB","Microsoft YaHei",sans-serif;background:var(--bg);color:var(--fg)}
header{padding:10px 16px;border-bottom:1px solid var(--line);display:flex;gap:12px;align-items:center;flex-wrap:wrap;background:var(--card)}
h1{font-size:15px;margin:0;font-weight:600}
#who{color:var(--mut)}
#who b{color:var(--fg)}
input{font:inherit;padding:5px 9px;border:1px solid var(--line);border-radius:6px;width:190px}
button{font:inherit;padding:5px 12px;border:1px solid var(--line);border-radius:6px;background:var(--card);cursor:pointer}
button:hover{border-color:var(--acc);color:var(--acc)}
button:disabled{opacity:.5;cursor:default}
main{display:flex;height:calc(100vh - 53px)}
#stage{flex:1;overflow:hidden;position:relative}
svg{width:100%;height:100%;display:block;cursor:grab}
svg.drag{cursor:grabbing}
#tip{position:absolute;pointer-events:none;background:var(--card);border:1px solid var(--line);border-radius:6px;padding:6px 9px;box-shadow:0 2px 10px rgba(0,0,0,.12);display:none;max-width:260px}
/* 坐标已按视口缩放过，这里 1 单位 = 1 CSS 像素，字号直接用 px */
svg text{font:11px/1 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;fill:#3a3a3c;paint-order:stroke;stroke:#fbfbfd;stroke-width:3px;stroke-linejoin:round}
svg text.rel{font-size:10px;fill:#6e6e73}
svg line{stroke:#b9b9c0;stroke-width:1}
svg line.par{stroke:#e3e3e8;stroke-dasharray:2 3}
svg circle{fill:#fff;stroke:#0a6cff;stroke-width:1.5;cursor:pointer}
svg .nd:hover circle{fill:#0a6cff}
aside{width:330px;border-left:1px solid var(--line);background:var(--card);overflow:auto;padding:16px;flex:none}
aside h2{font-size:16px;margin:0 0 4px}
.meta{color:var(--mut);font-size:12px;margin-bottom:14px}
aside h3{font-size:13px;margin:16px 0 6px;color:var(--mut);font-weight:600}
ul{margin:0;padding-left:18px}
li{margin:2px 0}
.req{color:#b3261e}
.sample{color:var(--mut);font-size:12px}
#msg{padding:6px 16px;color:var(--mut);border-top:1px solid var(--line);background:var(--card);font-size:12px}
.err{color:#b3261e}
</style>
</head>
<body>
<header>
  <h1>业务关系图</h1>
  <span id="who"></span>
  <input id="t" type="password" placeholder="访问口令" autocomplete="off">
  <button id="go">打开</button>
  <span id="status"></span>
</header>
<main>
  <div id="stage">
    <svg id="svg"></svg>
    <div id="tip"></div>
  </div>
  <aside id="panel"><p class="meta">点一个对象，看它的属性与说明。</p></aside>
</main>
<div id="msg"></div>
<script>
"use strict";
var TOKEN = "";          // 只存在内存里：不写进地址栏、不写进页面、不进浏览器存储
var DATA = null;
var $ = function (s) { return document.querySelector(s); };

function say(t, bad) { var e = $("#msg"); e.textContent = t; e.className = bad ? "err" : ""; }

$("#go").onclick = function () {
  TOKEN = $("#t").value.trim();
  $("#t").value = "";            // 用完就从输入框抹掉
  load();
};
$("#t").addEventListener("keydown", function (e) { if (e.code === "Enter") $("#go").click(); });

function load() {
  $("#status").textContent = "读取中…";
  var u = "api/graph?project=" + encodeURIComponent(who.project) + "&version=" + who.version;
  fetch(u, { headers: { Authorization: "Bearer " + TOKEN } })
    .then(function (r) { return r.json().then(function (j) { return { ok: r.ok, j: j }; }); })
    .then(function (res) {
      if (!res.ok) { $("#status").textContent = ""; say(res.j.error || "读不到", true); return; }
      DATA = res.j;
      $("#who").innerHTML = "项目 <b>" + esc(who.project) + "</b> · 第 <b>" + who.version + "</b> 版";
      $("#status").textContent = DATA.nodes.length + " 个对象 · " + DATA.edges.length + " 条关系";
      draw();
    })
    .catch(function (e) { $("#status").textContent = ""; say(String(e), true); });
}

var who = { project: "%%PROJECT%%", version: %%VERSION%% };

function esc(s) {
  return String(s == null ? "" : s).replace(/[&<>"']/g, function (c) {
    return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
  });
}

// ── 力导向：斥力 + 弹簧 + 向心，三段。91×91 两两比较在浏览器里是毫秒级 ──
//
// 参数是**实测调出来的**，不是拍的：判据是「最近邻距离」——
// 摊得太开标签就叠成一团，太挤就看不清。91 节点下这组给到 ~90 的最近邻距，
// 实测布局耗时 ~45ms（.review-workspace/req093/tune.js 是当时的对照台）。
function layout(ns, es, W, H) {
  var n = ns.length;
  ns.forEach(function (d, i) {
    var a = 2 * Math.PI * i / n;
    d.x = W / 2 + Math.cos(a) * Math.min(W, H) * 0.32;
    d.y = H / 2 + Math.sin(a) * Math.min(W, H) * 0.32;
    d.vx = 0; d.vy = 0;
  });
  var by = {}; ns.forEach(function (d) { by[d.id] = d; });
  for (var it = 0; it < 400; it++) {
    for (var i = 0; i < n; i++) {
      for (var j = i + 1; j < n; j++) {
        var a = ns[i], b = ns[j];
        var dx = b.x - a.x, dy = b.y - a.y;
        var d2 = dx * dx + dy * dy;
        if (d2 < 4) { dx = (i % 7) - 3 + 0.5; dy = (j % 5) - 2 + 0.5; d2 = dx * dx + dy * dy; }
        var f = 400 / d2, fx = dx * f, fy = dy * f;
        a.vx -= fx; a.vy -= fy; b.vx += fx; b.vy += fy;
      }
    }
    es.forEach(function (e) {
      var a = by[e.from], b = by[e.to]; if (!a || !b) return;
      var dx = b.x - a.x, dy = b.y - a.y, d = Math.sqrt(dx * dx + dy * dy) || 1;
      var f = (d - 60) * 0.05 / d, fx = dx * f, fy = dy * f;
      a.vx += fx; a.vy += fy; b.vx -= fx; b.vy -= fy;
    });
    ns.forEach(function (d) {
      d.vx += (W / 2 - d.x) * 0.04; d.vy += (H / 2 - d.y) * 0.04;
      d.vx *= 0.86; d.vy *= 0.86; d.x += d.vx; d.y += d.vy;
    });
  }
}

// 把布局结果**缩放平移**进视口。
//
// ⚠️ 这一步不是可有可无的美化：不缩放的话，力导向的坐标能跑到 ±6000，
// 而 SVG 没有 viewBox 时 1 用户单位 = 1 CSS 像素，**绝大多数点会落在视口外，
// 页面上等于一片空白**。
//
// 之所以缩放而不是设 viewBox：viewBox 会连**字号**一起缩放，
// 而布局是正方形的、视口是横的 —— `meet` 会按高度缩（0.42 倍），
// 14px 的标签只剩 6px。改成缩放坐标、按像素直接画，字号才可控可预期。
//
// ⚠️ 这条注释自己也踩了两次：原先写「按 CSS 像素一对一画」，
// 其中的「一对一」被第三档的**形态**守门判成未翻译的记号、整页被拒发；
// 改成「未翻译的关系记号」时又把那个词本身写进了注释，再被拒一次。
// 措辞改掉就是了 —— 守门器不看上下文，只看形状。
function fit(ns, W, H) {
  var pad = 44;
  var xs = ns.map(function (d) { return d.x; }), ys = ns.map(function (d) { return d.y; });
  var x0 = Math.min.apply(null, xs), x1 = Math.max.apply(null, xs);
  var y0 = Math.min.apply(null, ys), y1 = Math.max.apply(null, ys);
  var s = Math.min((W - 2 * pad) / Math.max(1, x1 - x0), (H - 2 * pad) / Math.max(1, y1 - y0));
  var cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
  ns.forEach(function (d) { d.x = (d.x - cx) * s + W / 2; d.y = (d.y - cy) * s + H / 2; });
}

function draw() {
  var svg = $("#svg"), W = svg.clientWidth || 1000, H = svg.clientHeight || 700;
  var ns = DATA.nodes.map(function (d) {
    return { id: d.id, label: d.label, c: d.attribute_count, p: d.parent };
  });
  layout(ns, DATA.edges, W, H);
  fit(ns, W, H);   // ← 缩放进视口；不调它页面几乎什么都看不见

  // 父对象 → 子对象的归属线（比关系线淡，排在底层）
  var by = {}; ns.forEach(function (d) { by[d.id] = d; });
  var parts = [];
  ns.forEach(function (d) {
    var p = d.p && by[d.p];
    if (p) parts.push('<line class="par" x1="' + p.x.toFixed(1) + '" y1="' + p.y.toFixed(1) + '" x2="' + d.x.toFixed(1) + '" y2="' + d.y.toFixed(1) + '"/>');
  });
  DATA.edges.forEach(function (e) {
    var a = by[e.from], b = by[e.to]; if (!a || !b) return;
    var mx = (a.x + b.x) / 2, my = (a.y + b.y) / 2;
    parts.push('<g><line x1="' + a.x.toFixed(1) + '" y1="' + a.y.toFixed(1) + '" x2="' + b.x.toFixed(1) + '" y2="' + b.y.toFixed(1) + '"/>' +
      '<text class="rel" x="' + mx.toFixed(1) + '" y="' + my.toFixed(1) + '">' + esc(e.label) + '</text></g>');
  });
  ns.forEach(function (d) {
    var r = 5 + Math.min(9, d.c * 0.45);
    parts.push('<g class="nd" data-id="' + esc(d.id) + '"><circle cx="' + d.x.toFixed(1) + '" cy="' + d.y.toFixed(1) + '" r="' + r.toFixed(1) + '"/>' +
      '<text x="' + d.x.toFixed(1) + '" y="' + (d.y - r - 4).toFixed(1) + '">' + esc(d.label) + '</text></g>');
  });
  svg.innerHTML = '<g>' + parts.join("") + '</g>';
  $("#tip").style.display = "none";

  svg.querySelectorAll(".nd").forEach(function (g) {
    g.addEventListener("click", function () { pick(g.getAttribute("data-id")); });
    g.addEventListener("mouseenter", function (ev) {
      var t = $("#tip");
      t.innerHTML = "<b>" + esc(by[g.getAttribute("data-id")].label) + "</b> · " + by[g.getAttribute("data-id")].c + " 项属性";
      t.style.display = "block";
      t.style.left = (ev.offsetX + 14) + "px"; t.style.top = (ev.offsetY + 14) + "px";
    });
    g.addEventListener("mouseleave", function () { $("#tip").style.display = "none"; });
  });
}

// 点节点 → 看它的属性与说明（客户视图里已有的 note / 属性）
function pick(id) {
  var u = "api/node?project=" + encodeURIComponent(who.project) + "&version=" + who.version + "&id=" + encodeURIComponent(id);
  fetch(u, { headers: { Authorization: "Bearer " + TOKEN } })
    .then(function (r) { return r.json().then(function (j) { return { ok: r.ok, j: j }; }); })
    .then(function (res) {
      var p = $("#panel");
      if (!res.ok) { p.innerHTML = '<p class="err">' + esc(res.j.error || "读不到") + "</p>"; return; }
      var d = res.j, h = "<h2>" + esc(d.name) + "</h2>";
      h += '<p class="meta">' + d.properties.length + " 项属性" + (d.aliases.length ? " · 又叫 " + esc(d.aliases.join("、")) : "") + "</p>";
      if (d.note) h += "<p>" + esc(d.note) + "</p>";
      if (d.properties.length) {
        h += "<h3>属性</h3><ul>";
        d.properties.forEach(function (x) {
          h += "<li>" + esc(x.name) + (x.required ? ' <span class="req">必填</span>' : "") +
            (x.sample ? ' <span class="sample">例：' + esc(x.sample) + "</span>" : "") + "</li>";
        });
        h += "</ul>";
      }
      p.innerHTML = h;
    });
}

say("填入口令后点「打开」。");
</script>
</body>
</html>
"##;

/// 把页面里的项目名与版本号填进去。
///
/// 用**一次性替换**而不是 `format!`：`format!` 要把整段 HTML 当格式串，
/// 而 JS 里的 `{}`（对象字面量、函数体）会让它当场炸掉。
/// 替换用的占位符也刻意选成 CSS/JS 里不可能出现的 `%%…%%`。
///
/// `pub`（不是 `pub(crate)`）：`tests/test_graph_server.rs` 要直接
/// 断言渲染出来的页面过守门器、且不引任何外部资源。
pub fn index_html(project: &str, version: i32) -> String {
    INDEX_HTML
        .replace("%%PROJECT%%", project)
        .replace("%%VERSION%%", &version.to_string())
}
