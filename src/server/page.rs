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
//! # ⭐ REQ-110 · 本页是**静态壳**，壳里零数据（R4 / AC-005）
//!
//! 收口前 `index_html(project, version)` 把项目名与版本渲染进 JS 字面量，
//! 于是「页面壳」本身就是一条数据出口 —— 而 `/` 又被身份门拦着，浏览器永远
//! 打不开它。REQ-110 把两件事一起改掉：
//!
//! | | 收口前 | 现在 |
//! | --- | --- | --- |
//! | `/` 的 token | 必须 | **免** |
//! | 壳里的数据 | 项目名 + 版本 | **零**（连数字都不渲染） |
//! | 作用域从哪来 | 服务端注入 | 拿到口令后 `GET /api/scope`（gated） |
//! | TTL 导出 | 没有 | 「导出 TTL」按钮 → `GET /api/ontology.ttl`（gated） |
//!
//! ⇒ 判据「`GET /` 的响应体里不得出现项目名 / 版本号 / 任何对象名」
//! 由 `src/server/http.rs` 的 `ac005_the_shell_carries_no_data_for_secretproj`
//! 在**真路由**上钉住（同一个 `#[cfg(test)]` 模块里还有 `ac006_every_data_api_is_still_gated`）。
//!
//! # ⭐ REQ-114 · 从「算完就定住」改成「能手动整理」
//!
//! 用户原话：「html ui 有问题，太简陋了，graph 的点也无法拖动，导致重叠遮挡。」
//! 改法**全部**落在下面这一份内联 JS/CSS 里（仍然零依赖、页面仍 < 60KB）：
//!
//! | AC | 落点（本文件内的函数 / 常量） |
//! | --- | --- |
//! | 拖拽 · 松手固定 · 双击释放 | `startDrag` / `onUp` / `release` |
//! | 碰撞消解（最小间距**位置**推离） | `collide`（物理步的**最后**一步，每帧两遍） |
//! | 拖拽 ≠ 点击（4px） | `DRAG_PX` + `onUp` 里的 `DRAG.moved` |
//! | 适应窗口 / 重置 | `fitView` / `resetView` |
//! | 标签避让（`k < 0.6` 隐藏 + 重叠跳过） | `LBL_K` + `paint` 的避让循环 |
//! | 搜索高亮 + 回车定位 | `#q` 的两个监听 + `centerOn` |
//! | 视觉（大小 / 箭头 / 图例 / 悬停 / 暗色） | `radiusOf` / `<marker>` / `#legend` / `showTip` / `@media` |
//!
//! ## 力导向**必须收敛**（REQ-114 硬约束）
//!
//! 老版本是一次性 400 轮、算完就固定。现在改成动画：`requestAnimationFrame`
//! 逐帧推进，但**上限 `SIM_FRAMES` = 300 帧**且 `alpha` 每帧 ×0.97 ——
//! 两个条件任一满足就**停帧**（`SIM_FRAMES` / `ALPHA_MIN`）。
//! ⇒ 稳态时**一个 rAF 都不在跑**，不存在「永远满帧」。
//! 只有「拖拽进行中」才会为了每帧碰撞消解而临时继续跑帧。
//!
//! ## 本机**没有 headless 浏览器** ⇒ 只有源码级判据
//!
//! ⛔ 交付物里**不许**写「已验证交互流畅」这类无法证的结论。REQ-114 的验证分三层：
//!
//! 1. **源码级**（可持续、可复跑）：事件绑定、阈值常量、关键函数都在本文件里，
//!    逐条列在交付物里（函数名 + 行号）；
//! 2. **纯函数级**（真跑）：布局内核 `radiusOf` / `collide` / `physicsStep` 不碰 DOM，
//!    被 `.review-workspace/req114/layout_probe.js` 原样抽出在 Node 里推进 300 帧，
//!    断言 AC-002 的数据级判据「任意两节点中心距 ≥ 半径和」；
//! 3. **真浏览器手测：未做**（本机无 headless 浏览器，也不许为此加依赖）。
//!
//! # 布局为什么手写
//!
//! 力导向的核心就是「斥力 + 弹簧 + 向心 + 碰撞」四段，83 个节点 / 52 条边
//! 两两斥力是 3403 对 × 300 帧 ≈ 102 万次 —— 浏览器里毫秒级，
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
//!
//! 同一条纪律对 REQ-114 的新代码继续生效：变量名里不许出现 `key` / `pk`，
//! 也不许写出 `单字符:单字符` 形态（那是基数记号，第三档守门按**形状**抓）。

/// 页面本体。**注意**：这里出现的每一个词都会被守门器扫过。
///
/// ⛔ **本常量里不许出现任何项目名 / 版本号 / 对象名** —— `/` 是免 token 的
/// 静态壳（REQ-110 · AC-005）。要数据，页面自己去 gated API 拿。
pub(crate) const INDEX_HTML: &str = r##"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>业务关系图</title>
<style>
:root{--bg:#fbfbfd;--fg:#1d1d1f;--mut:#6e6e73;--line:#c7c7cc;--acc:#0a6cff;--card:#fff;--edge:#b9b9c0;--hit:#ff9500;--top:#0a6cff;--sub:#0f9d8f;--shadow:0 2px 10px rgba(0,0,0,.12)}
@media (prefers-color-scheme: dark){
  :root{--bg:#141416;--fg:#f2f2f7;--mut:#9b9ba1;--line:#3c3c40;--acc:#4d9bff;--card:#1c1c1f;--edge:#5c5c63;--hit:#ffb340;--top:#4d9bff;--sub:#2fc4b2;--shadow:0 2px 10px rgba(0,0,0,.55)}
}
*{box-sizing:border-box}
body{margin:0;font:14px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Hiragino Sans GB","Microsoft YaHei",sans-serif;background:var(--bg);color:var(--fg)}
header{padding:8px 16px;border-bottom:1px solid var(--line);display:flex;gap:10px;align-items:center;flex-wrap:wrap;background:var(--card)}
h1{font-size:15px;margin:0;font-weight:600}
#who{color:var(--mut)}
#who b{color:var(--fg)}
input{font:inherit;padding:5px 9px;border:1px solid var(--line);border-radius:6px;width:170px;background:var(--card);color:var(--fg)}
#q{width:210px}
button{font:inherit;padding:5px 12px;border:1px solid var(--line);border-radius:6px;background:var(--card);color:var(--fg);cursor:pointer}
button:hover{border-color:var(--acc);color:var(--acc)}
button:disabled{opacity:.5;cursor:default}
main{display:flex;height:calc(100vh - 51px)}
#stage{flex:1;overflow:hidden;position:relative;background:var(--bg)}
#stage svg{width:100%;height:100%;display:block;cursor:grab;touch-action:none}
#stage svg.drag{cursor:grabbing}
#stage svg g.nd{cursor:grab}
#stage svg g.nd.dragging{cursor:grabbing}
/* 坐标已按视口缩放过：1 世界单位 = 1 CSS 像素（默认 k=1），字号直接用 px */
#stage svg text.lb{font:11px/1 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;fill:var(--fg);text-anchor:middle;paint-order:stroke;stroke:var(--bg);stroke-width:3px;stroke-linejoin:round}
#stage svg text.rl{font:10px/1 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;fill:var(--mut);text-anchor:middle;paint-order:stroke;stroke:var(--bg);stroke-width:3px;stroke-linejoin:round}
#stage svg line.par{stroke:var(--line);stroke-width:1;stroke-dasharray:3 4;vector-effect:non-scaling-stroke}
#stage svg g.rel line{stroke:var(--edge);stroke-width:1.2;vector-effect:non-scaling-stroke}
#arw path{fill:var(--edge)}
#stage svg g.nd circle{fill:var(--card);stroke:var(--top);stroke-width:1.6;vector-effect:non-scaling-stroke}
#stage svg g.nd.sub circle{stroke:var(--sub)}
#stage svg g.nd:hover circle{stroke-width:3}
#stage svg g.nd.hit circle{stroke:var(--hit);stroke-width:3.2}
#stage svg g.nd.hit text.lb{fill:var(--hit);font-weight:700}
#stage svg g.nd.dim{opacity:.35}
#stage svg g.nd.nolbl text.lb{display:none}
#tip{position:absolute;left:0;top:0;pointer-events:none;background:var(--card);border:1px solid var(--line);border-radius:6px;padding:6px 9px;box-shadow:var(--shadow);display:none;max-width:280px;font-size:12px;line-height:1.5;z-index:3}
#legend{position:absolute;left:12px;bottom:12px;background:var(--card);border:1px solid var(--line);border-radius:8px;padding:8px 11px;font-size:12px;color:var(--mut);line-height:1.7;box-shadow:var(--shadow);max-width:340px;z-index:2}
#legend b{color:var(--fg);font-weight:600}
#legend i{display:inline-block;vertical-align:middle;margin-right:6px}
#legend .lg-big{width:14px;height:14px;border-radius:50%;border:2px solid var(--top);background:var(--card)}
#legend .lg-sm{width:8px;height:8px;border-radius:50%;border:2px solid var(--sub);background:var(--card)}
#legend .lg-line{width:18px;height:0;border-top:2px solid var(--edge)}
#legend .lg-dash{width:18px;height:0;border-top:2px dashed var(--edge)}
#legend .kbd{margin-top:6px;color:var(--mut);font-size:11px}
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
  <input id="q" placeholder="查找对象名（回车定位）" autocomplete="off">
  <button id="fit">适应窗口</button>
  <button id="reset">重置</button>
  <button id="export" disabled>导出 TTL</button>
  <span id="status"></span>
</header>
<main>
  <div id="stage">
    <svg id="svg"></svg>
    <div id="tip"></div>
    <div id="legend">
      <b>怎么看</b>
      <div><i class="lg-big"></i>圆圈越大 = 属性越多（<i class="lg-sm"></i>小圈 = 属性少）</div>
      <div><i class="lg-line"></i>箭头 = 关系方向，线上的字 = 关系的说法</div>
      <div><i class="lg-dash"></i>虚线 = 所属关系（谁属于谁）</div>
      <div class="kbd">滚轮缩放 · 拖背景平移 · 拖节点手动整理（松手即固定）· 双击节点释放 · 缩小到 0.6 倍以下自动省掉小字</div>
    </div>
  </div>
  <aside id="panel"><p class="meta">点一个对象，看它的属性与说明。</p></aside>
</main>
<div id="msg"></div>
<script>
"use strict";
var TOKEN = "";          // 只存在内存里：不写进地址栏、不写进页面、不进浏览器存储
var SCOPE = null;        // 作用域**不在壳里**：拿到口令后问 gated API
var DATA = null;
var $ = function (s) { return document.querySelector(s); };

// 视图：屏幕坐标 = 视口中心 + (VIEW.x, VIEW.y) + 世界坐标 × VIEW.k
// 默认 k=1 / x=0 / y=0 —— 「重置」回到的正是这个初始视野（AC-004）。
var VIEW = { k: 1, x: 0, y: 0 };
var NS = [], ES = [], PS = [], BY = {}, TOUCHED = false;
var world = null, parEls = [], relEls = [], relLines = [], relTxts = [], ndEls = [];
var DRAG = null, PAN = null, raf = 0, frames = 0, alpha = 0;

// ⭐ 交互阈值（AC-003 / AC-005 的判据常量就在这两行）
var DRAG_PX = 4;         // 位移 > 4px ⇒ 这一串按下-松开**不算点击**
var LBL_K = 0.6;         // k < 0.6 ⇒ 隐藏节点标签（避免密集处文字压住）
var REL_K = 0.85;        // k < 0.85 ⇒ 隐藏关系线上的字（线还在，方向还在）

function say(t, bad) { var e = $("#msg"); e.textContent = t; e.className = bad ? "err" : ""; }
function ask(path) { return fetch(path, { headers: { Authorization: "Bearer " + TOKEN } }); }

$("#go").onclick = function () {
  TOKEN = $("#t").value.trim();
  $("#t").value = "";            // 用完就从输入框抹掉
  open();
};
$("#t").addEventListener("keydown", function (e) { if (e.code === "Enter") $("#go").click(); });
$("#export").onclick = save_ttl;
$("#fit").onclick = fitView;
$("#reset").onclick = resetView;

// ─── 布局内核（纯函数、不碰 DOM）─────────────────────────────
// 这一段被 `.review-workspace/req114/layout_probe.js` **原样抽出**在 Node 里跑：
// 83 个节点全部放在同一坐标、推进 300 帧，断言「任意两节点中心距 ≥ 半径和」
// （AC-002 的数据级判据）。改这一段就要重跑那个探针。
var GAP = 6;             // 碰撞消解的最小间隙（世界单位）
var SIM_FRAMES = 300;    // 力导向最多 300 帧 —— 必须收敛，不许永远满帧
var ALPHA_MIN = 0.006;   // alpha 掉到它以下就停帧
var REP = 400;           // 斥力系数（近处强、远处几乎为零 ⇒ 局部撑开、全局靠向心收）
var DAMP = 0.82;         // 速度阻尼
var VMAX = 24;           // 单帧速度上限（防爆冲）
var CENTER_F = 0.4;      // 向心系数（把整张图收进一屏；实测这组参数 83 节点收在 ~550×550）
var LINK_LEN = 110;      // 关系边的理想长度
var PAR_LEN = 90;        // 所属连线的理想长度

// 节点半径按属性数走（视觉升级第 1 条）：6 起步，上限 16。
function radiusOf(c) {
  var n = Math.max(0, Number(c) || 0);
  return 6 + Math.min(10, Math.sqrt(n) * 3.2);
}

// 碰撞消解（AC-002）：对「距离 < 半径和 + 间隙」的节点对做**位置**推离。
// 不是靠斥力慢慢挤 —— 斥力在 alpha 收敛后就没有位移了，重叠会永远留着。
// 被钉住的节点权重 0（它不动，让对面让开），否则拖拽会被下一帧弹回去。
function collide(ns, gap) {
  for (var i = 0; i < ns.length; i++) {
    var a = ns[i];
    for (var j = i + 1; j < ns.length; j++) {
      var b = ns[j];
      var dx = b.x - a.x, dy = b.y - a.y;
      var d = Math.sqrt(dx * dx + dy * dy);
      var min = a.r + b.r + gap;
      if (d >= min) continue;
      if (d < 1e-4) { dx = 1; dy = ((i + j) % 7) - 3; d = Math.sqrt(dx * dx + dy * dy); }
      var push = min - d;
      var wa = a.pin ? 0 : (b.pin ? 1 : 0.5);
      var wb = b.pin ? 0 : (a.pin ? 1 : 0.5);
      a.x -= dx / d * push * wa; a.y -= dy / d * push * wa;
      b.x += dx / d * push * wb; b.y += dy / d * push * wb;
    }
  }
}

// 弹簧：关系边拉长、所属连线稍短一点（两种线各一套参数）。
function springs(list, len, k) {
  for (var i = 0; i < list.length; i++) {
    var a = list[i].a, b = list[i].b;
    var dx = b.x - a.x, dy = b.y - a.y;
    var d = Math.sqrt(dx * dx + dy * dy) || 1;
    var w = (d - len) * k / d;
    a.vx += dx * w; a.vy += dy * w;
    b.vx -= dx * w; b.vy -= dy * w;
  }
}

// 一帧 = 斥力 → 弹簧 → 向心/阻尼 → 位移 → **碰撞消解（最后一步，跑两遍）**
// 碰撞放最后跑两遍，是为了让「刚被推开的节点」不会在下一帧把别人顶回去。
function physicsStep(ns, es, ps, alpha) {
  var i, j, a, b, dx, dy, d2, f, sp;
  for (i = 0; i < ns.length; i++) {
    a = ns[i];
    for (j = i + 1; j < ns.length; j++) {
      b = ns[j];
      dx = b.x - a.x; dy = b.y - a.y; d2 = dx * dx + dy * dy;
      if (d2 < 1) { dx = ((i % 7) - 3) * 0.7 + 0.35; dy = ((j % 5) - 2) * 0.7 + 0.35; d2 = dx * dx + dy * dy; }
      if (d2 < 25) d2 = 25;
      f = REP / d2;
      a.vx -= dx * f; a.vy -= dy * f;
      b.vx += dx * f; b.vy += dy * f;
    }
  }
  springs(es, LINK_LEN, 0.04);
  springs(ps, PAR_LEN, 0.07);
  for (i = 0; i < ns.length; i++) {
    a = ns[i];
    a.vx -= a.x * CENTER_F; a.vy -= a.y * CENTER_F;
    a.vx *= DAMP; a.vy *= DAMP;
    sp = Math.sqrt(a.vx * a.vx + a.vy * a.vy);
    if (sp > VMAX) { a.vx *= VMAX / sp; a.vy *= VMAX / sp; }
    if (a.pin) { a.vx = 0; a.vy = 0; continue; }
    a.x += a.vx * alpha; a.y += a.vy * alpha;
  }
  collide(ns, GAP);
  collide(ns, GAP);
}
// ─── 布局内核结束 ───────────────────────────────────────────

function stageBox() {
  var s = $("#svg");
  return { w: s.clientWidth || 1000, h: s.clientHeight || 700 };
}

function toWorld(ev) {
  var box = stageBox(), r = $("#svg").getBoundingClientRect();
  return {
    x: (ev.clientX - r.left - box.w / 2 - VIEW.x) / VIEW.k,
    y: (ev.clientY - r.top - box.h / 2 - VIEW.y) / VIEW.k
  };
}

// 连线两端各让开一个半径（不然箭头扎进圈里、线从圆心长出来）。
function trim(a, b) {
  var dx = b.x - a.x, dy = b.y - a.y;
  var d = Math.sqrt(dx * dx + dy * dy) || 1;
  var ux = dx / d, uy = dy / d;
  var ra = Math.min(a.r, d * 0.45), rb = Math.min(b.r + 3, d * 0.45);
  return { x1: a.x + ux * ra, y1: a.y + uy * ra, x2: b.x - ux * rb, y2: b.y - uy * rb };
}

// ─── 帧循环：收敛即停（硬约束 5）────────────────────────────
function kick(a) {
  alpha = a;
  frames = 0;
  if (!raf) raf = requestAnimationFrame(loop);
}

function loop() {
  raf = 0;
  physicsStep(NS, ES, PS, DRAG ? 0.45 : alpha);
  paint();
  frames++;
  alpha = alpha * 0.97;
  if (DRAG) { raf = requestAnimationFrame(loop); return; }              // 拖拽中：每帧都消解
  if (frames < SIM_FRAMES && alpha > ALPHA_MIN) { raf = requestAnimationFrame(loop); return; }
  // 收敛：停帧 + （用户没动过视图时）把整张图放大到接近铺满，使 k=1/x=0/y=0 就是默认视野
  if (!TOUCHED) { normalizeWorld(); VIEW = { k: 1, x: 0, y: 0 }; paint(); }
}

function normalizeWorld() {
  if (!NS.length) return;
  var box = stageBox(), pad = 56, i, d;
  var x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
  for (i = 0; i < NS.length; i++) {
    d = NS[i];
    if (d.x - d.r < x0) x0 = d.x - d.r;
    if (d.x + d.r > x1) x1 = d.x + d.r;
    if (d.y - d.r < y0) y0 = d.y - d.r;
    if (d.y + d.r > y1) y1 = d.y + d.r;
  }
  var cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
  var s = Math.min((box.w - 2 * pad) / Math.max(1, x1 - x0), (box.h - 2 * pad) / Math.max(1, y1 - y0));
  // ⚠️ **只放大、不缩小**（下限 1）：缩小坐标而半径不变，等于把已经分开的点又挤到一起
  //    —— 而这时帧循环已经停了，碰撞消解不会再跑。窗口太小时宁可由「适应窗口」去缩视图。
  s = Math.max(1, Math.min(1.5, s));
  for (i = 0; i < NS.length; i++) {
    d = NS[i];
    d.x = (d.x - cx) * s; d.y = (d.y - cy) * s;
  }
}

// AC-004：「适应窗口」—— 按当前节点分布重算 k 与中心。
function fitView() {
  if (!NS.length) { say("先填口令点「打开」，图上还没有对象。", true); return; }
  TOUCHED = true;
  var box = stageBox(), pad = 56, i, d;
  var x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
  for (i = 0; i < NS.length; i++) {
    d = NS[i];
    if (d.x - d.r < x0) x0 = d.x - d.r;
    if (d.x + d.r > x1) x1 = d.x + d.r;
    if (d.y - d.r < y0) y0 = d.y - d.r;
    if (d.y + d.r > y1) y1 = d.y + d.r;
  }
  var cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
  var k = Math.min((box.w - 2 * pad) / Math.max(1, x1 - x0), (box.h - 2 * pad) / Math.max(1, y1 - y0));
  VIEW.k = Math.max(0.15, Math.min(4, k));
  VIEW.x = -cx * VIEW.k;
  VIEW.y = -cy * VIEW.k;
  paint();
}

// AC-004：「重置」—— 回到默认视野 k=1 / x=0 / y=0（不动节点位置，只复位视图）。
function resetView() {
  TOUCHED = true;
  VIEW = { k: 1, x: 0, y: 0 };
  paint();
  say("视图已重置（k=1, x=0, y=0）。", false);
}

// AC-006：回车定位 —— 把这个节点平移到视口中心。
function centerOn(d) {
  TOUCHED = true;
  if (VIEW.k < LBL_K) VIEW.k = 1;      // 太小的时候先放大，否则定位过去也看不清
  VIEW.x = -d.x * VIEW.k;
  VIEW.y = -d.y * VIEW.k;
  paint();
  d.el.classList.add("hit");
  setTimeout(function () { if (!d.hit) d.el.classList.remove("hit"); }, 1200);
}

// ─── 绘制 ───────────────────────────────────────────────────
function buildScene() {
  var i, out = [];
  out.push('<defs><marker id="arw" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M0,0 L10,5 L0,10 z"/></marker></defs>');
  out.push('<g id="world">');
  for (i = 0; i < PS.length; i++) out.push('<line class="par" x1="0" y1="0" x2="0" y2="0"/>');
  for (i = 0; i < ES.length; i++) out.push('<g class="rel"><line x1="0" y1="0" x2="0" y2="0" marker-end="url(#arw)"/><text class="rl"></text></g>');
  for (i = 0; i < NS.length; i++) {
    out.push('<g class="nd' + (NS[i].p ? " sub" : "") + '" data-id="' + esc(NS[i].id) + '">' +
      '<circle r="' + NS[i].r.toFixed(1) + '"/>' +
      '<text class="lb" y="' + (-NS[i].r - 5).toFixed(1) + '"></text></g>');
  }
  out.push('</g>');
  $("#svg").innerHTML = out.join("");
  world = $("#world");
  parEls = [].slice.call(world.querySelectorAll("line.par"));
  relEls = [].slice.call(world.querySelectorAll("g.rel"));
  relLines = relEls.map(function (g) { return g.querySelector("line"); });
  relTxts = relEls.map(function (g) { return g.querySelector("text"); });
  ndEls = [].slice.call(world.querySelectorAll("g.nd"));
  for (i = 0; i < ES.length; i++) relTxts[i].textContent = ES[i].label;
  for (i = 0; i < NS.length; i++) {
    NS[i].el = ndEls[i];
    ndEls[i].querySelector("text").textContent = NS[i].label;
    bindNode(NS[i], ndEls[i]);
  }
}

function paint() {
  if (!world) return;
  var box = stageBox(), i, d, k = VIEW.k, showRel = k >= REL_K, showLbl = k >= LBL_K;
  world.setAttribute("transform",
    "translate(" + (box.w / 2 + VIEW.x).toFixed(1) + "," + (box.h / 2 + VIEW.y).toFixed(1) +
    ") scale(" + k.toFixed(4) + ")");

  // ① 所属连线（淡、虚线、排底层）
  for (i = 0; i < PS.length; i++) {
    var p = PS[i], t = trim(p.a, p.b), pl = parEls[i];
    pl.setAttribute("x1", t.x1.toFixed(1)); pl.setAttribute("y1", t.y1.toFixed(1));
    pl.setAttribute("x2", t.x2.toFixed(1)); pl.setAttribute("y2", t.y2.toFixed(1));
  }
  // ② 关系连线 + 方向箭头 + 关系说法
  for (i = 0; i < ES.length; i++) {
    var q = trim(ES[i].a, ES[i].b), ln = relLines[i], tx = relTxts[i];
    ln.setAttribute("x1", q.x1.toFixed(1)); ln.setAttribute("y1", q.y1.toFixed(1));
    ln.setAttribute("x2", q.x2.toFixed(1)); ln.setAttribute("y2", q.y2.toFixed(1));
    if (showRel) {
      tx.setAttribute("x", ((q.x1 + q.x2) / 2).toFixed(1));
      tx.setAttribute("y", ((q.y1 + q.y2) / 2 - 5).toFixed(1));
    }
    tx.style.display = showRel ? "" : "none";
  }
  // ③ 节点位置（整体平移由 #world 的 transform 承担，这里只写自身坐标）
  for (i = 0; i < NS.length; i++) {
    d = NS[i];
    d.el.setAttribute("transform", "translate(" + d.x.toFixed(1) + "," + d.y.toFixed(1) + ")");
  }
  // ④ 标签避让（AC-005）：k < LBL_K 一律不显示；够清晰时再按「搜索命中优先、
  //    然后属性多的优先」排一遍，重叠的后面的跳过 —— 宁可少几个字，不要糊成一片。
  var order = [];
  for (i = 0; i < NS.length; i++) order.push(i);
  order.sort(function (x, y) {
    var hd = (NS[y].hit ? 1 : 0) - (NS[x].hit ? 1 : 0);
    if (hd) return hd;
    return (NS[y].c - NS[x].c) || (x - y);
  });
  var boxes = [];
  for (i = 0; i < order.length; i++) {
    d = NS[order[i]];
    var show = showLbl;
    if (show) {
      var w = d.label.length * 11 + 6, h = 15;
      var bx = d.x - w / 2, by = d.y - d.r - 19;
      for (var j = 0; j < boxes.length; j++) {
        var o = boxes[j];
        if (!(bx + w < o.x || bx > o.x + o.w || by + h < o.y || by > o.y + o.h)) { show = false; break; }
      }
      if (show) boxes.push({ x: bx, y: by, w: w, h: h });
    }
    d.el.classList.toggle("nolbl", !show);
  }
}

// ─── 悬停提示 ───────────────────────────────────────────────
function showTip(d, ev) {
  if (DRAG) return;
  var deg = 0;
  for (var i = 0; i < ES.length; i++) if (ES[i].a === d || ES[i].b === d) deg++;
  var h = "<b>" + esc(d.label) + "</b><br>" + d.c + " 项属性 · " + deg + " 条关系";
  if (d.p && BY[d.p]) h += "<br>所属：" + esc(BY[d.p].label);
  var t = $("#tip");
  t.innerHTML = h;
  t.style.display = "block";
  moveTip(ev);
}
function moveTip(ev) {
  var t = $("#tip");
  if (t.style.display !== "block") return;
  var box = stageBox(), s = $("#stage").getBoundingClientRect();
  var x = Math.min(ev.clientX - s.left + 14, box.w - 270);
  var y = Math.min(ev.clientY - s.top + 14, box.h - 80);
  t.style.left = Math.max(4, x) + "px";
  t.style.top = Math.max(4, y) + "px";
}
function hideTip() { $("#tip").style.display = "none"; }

// ─── 拖拽 / 平移 / 缩放 ─────────────────────────────────────
function bindNode(d, g) {
  // AC-001：拖拽入口**绑在节点本身**（背景上的 pointerdown 只负责平移）
  g.addEventListener("pointerdown", function (ev) { startDrag(d, ev); });
  // AC-001：双击 ⇒ 释放固定，回到力导向
  g.addEventListener("dblclick", function (ev) { ev.preventDefault(); release(d); });
  g.addEventListener("pointerenter", function (ev) { showTip(d, ev); });
  g.addEventListener("pointermove", moveTip);
  g.addEventListener("pointerleave", hideTip);
}

function startDrag(d, ev) {
  ev.preventDefault();
  ev.stopPropagation();
  hideTip();
  var w = toWorld(ev);
  DRAG = { d: d, wasPin: d.pin, moved: false, sx: ev.clientX, sy: ev.clientY, ox: w.x - d.x, oy: w.y - d.y };
  d.pin = true;                 // 拖的时候就钉住，松手后继续保持（AC-001）
  d.vx = 0; d.vy = 0;
  d.el.classList.add("dragging");
  $("#svg").classList.add("drag");
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("pointercancel", onUp);
  if (!raf) raf = requestAnimationFrame(loop);   // 拖拽期间每帧都跑碰撞消解
}

function onMove(ev) {
  if (DRAG) {
    var dx = ev.clientX - DRAG.sx, dy = ev.clientY - DRAG.sy;
    // AC-003：位移超过 DRAG_PX 就不再算点击
    if (!DRAG.moved && Math.sqrt(dx * dx + dy * dy) > DRAG_PX) DRAG.moved = true;
    var w = toWorld(ev);
    DRAG.d.x = w.x - DRAG.ox;
    DRAG.d.y = w.y - DRAG.oy;
    DRAG.d.vx = 0; DRAG.d.vy = 0;
    TOUCHED = true;
    if (!raf) raf = requestAnimationFrame(loop);
    else paint();
  } else if (PAN) {
    TOUCHED = true;
    VIEW.x = PAN.x + (ev.clientX - PAN.sx);
    VIEW.y = PAN.y + (ev.clientY - PAN.sy);
    paint();
  }
}

function onUp() {
  window.removeEventListener("pointermove", onMove);
  window.removeEventListener("pointerup", onUp);
  window.removeEventListener("pointercancel", onUp);
  var d = DRAG ? DRAG.d : null;
  if (d) {
    d.el.classList.remove("dragging");
    if (DRAG.moved) {
      // ⭐ R3：松手**保持固定** —— pin 永远是 true，后面的帧不会把位置冲掉
      d.pin = true;
      say("「" + d.label + "」已固定；双击它可释放回自动布局。", false);
    } else {
      // 位移 ≤ DRAG_PX ⇒ 这是一次点击（AC-003 的另一半：拖完不弹详情）
      d.pin = DRAG.wasPin;
      pick(d.id);
    }
  }
  DRAG = null;
  PAN = null;
  $("#svg").classList.remove("drag");
  paint();
}

// AC-001：双击释放固定，回到力导向。
function release(d) {
  if (!d.pin) { say("「" + d.label + "」本来就没被固定。", false); return; }
  d.pin = false;
  d.vx = 0; d.vy = 0;
  TOUCHED = true;
  say("「" + d.label + "」已释放，回到自动布局。", false);
  kick(0.55);
}

// 背景按下 ⇒ 平移（与节点拖拽按事件目标区分，风险表第 1 条）
$("#svg").addEventListener("pointerdown", function (ev) {
  if (!NS.length) return;
  if (ev.target.closest && ev.target.closest(".nd")) return;
  hideTip();
  PAN = { sx: ev.clientX, sy: ev.clientY, x: VIEW.x, y: VIEW.y };
  $("#svg").classList.add("drag");
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
});

// 滚轮缩放：以指针下的世界点为锚（缩放时手感不飘）
$("#svg").addEventListener("wheel", function (ev) {
  if (!NS.length) return;
  ev.preventDefault();
  var box = stageBox(), r = $("#svg").getBoundingClientRect();
  var px = ev.clientX - r.left - box.w / 2, py = ev.clientY - r.top - box.h / 2;
  var wx = (px - VIEW.x) / VIEW.k, wy = (py - VIEW.y) / VIEW.k;
  var k = VIEW.k * Math.exp(-ev.deltaY * 0.0016);
  VIEW.k = Math.max(0.15, Math.min(4, k));
  VIEW.x = px - wx * VIEW.k;
  VIEW.y = py - wy * VIEW.k;
  TOUCHED = true;
  paint();
}, { passive: false });

window.addEventListener("resize", function () { paint(); });

// ─── 搜索：输入即高亮，回车即定位（AC-006）───────────────
$("#q").addEventListener("input", function () {
  var q = this.value.trim().toLowerCase();
  for (var i = 0; i < NS.length; i++) {
    var d = NS[i];
    d.hit = q.length > 0 && d.label.toLowerCase().indexOf(q) >= 0;
    if (d.el) {
      d.el.classList.toggle("hit", d.hit);
      d.el.classList.toggle("dim", q.length > 0 && !d.hit);
    }
  }
  paint();
});
$("#q").addEventListener("keydown", function (ev) {
  if (ev.code !== "Enter") return;
  var q = this.value.trim().toLowerCase();
  if (!q) return;
  var first = null;
  for (var i = 0; i < NS.length; i++) {
    if (NS[i].label.toLowerCase().indexOf(q) >= 0) { first = NS[i]; break; }
  }
  if (!first) { say("没有名称含「" + this.value.trim() + "」的对象。", true); return; }
  say("已定位到「" + first.label + "」。", false);
  centerOn(first);
});

// 第一步：问清「看哪个项目、哪一版」。这是**唯一**把作用域带进页面的路，
// 而且它要口令 —— 壳里永远没有这两个值。
function open() {
  $("#status").textContent = "读取中…";
  ask("api/scope")
    .then(function (r) { return r.json().then(function (j) { return { ok: r.ok, j: j }; }); })
    .then(function (res) {
      if (!res.ok) { $("#status").textContent = ""; say(res.j.error || "读不到", true); return; }
      SCOPE = res.j;
      $("#export").disabled = false;
      load();
    })
    .catch(function (e) { $("#status").textContent = ""; say(String(e), true); });
}

function load() {
  $("#status").textContent = "读取中…";
  var u = "api/graph?project=" + encodeURIComponent(SCOPE.project) + "&version=" + SCOPE.version;
  ask(u)
    .then(function (r) { return r.json().then(function (j) { return { ok: r.ok, j: j }; }); })
    .then(function (res) {
      if (!res.ok) { $("#status").textContent = ""; say(res.j.error || "读不到", true); return; }
      DATA = res.j;
      $("#who").innerHTML = "项目 <b>" + esc(SCOPE.project) + "</b> · 第 <b>" + esc(SCOPE.version) + "</b> 版";
      $("#status").textContent = DATA.nodes.length + " 个对象 · " + DATA.edges.length + " 条关系";
      draw();
    })
    .catch(function (e) { $("#status").textContent = ""; say(String(e), true); });
}

// 导出 TTL：带口令拉 gated 端点，用 blob 触发下载。
// 文件名从 Content-Disposition 里取（缺了才退回一个固定名）。
function save_ttl() {
  if (!SCOPE) { say("先填口令点「打开」。", true); return; }
  $("#status").textContent = "导出中…";
  fetch("api/ontology.ttl", { headers: { Authorization: "Bearer " + TOKEN } })
    .then(function (r) {
      if (!r.ok) {
        return r.json().then(function (j) { throw new Error(j.error || "导出失败"); });
      }
      return grab_name(r).then(function (name) {
        return r.blob().then(function (b) {
          var url = URL.createObjectURL(b);
          var a = document.createElement("a");
          a.href = url; a.download = name;
          document.body.appendChild(a); a.click(); a.remove();
          setTimeout(function () { URL.revokeObjectURL(url); }, 0);
          $("#status").textContent = "已导出 " + name;
        });
      });
    })
    .catch(function (e) { $("#status").textContent = ""; say("导出失败：" + e.message, true); });
}

function grab_name(r) {
  var cd = r.headers.get("Content-Disposition") || "";
  var m = /filename="([^"]+)"/.exec(cd);
  return Promise.resolve(m ? m[1] : "ontology.ttl");
}

function esc(s) {
  return String(s == null ? "" : s).replace(/[&<>"']/g, function (c) {
    return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
  });
}

// 数据到手 → 建运行时节点 / 边 → 起帧。
//
// 起手放在一个圆周上（而不是全叠在圆心）：全叠在一起时斥力方向要靠抖动打破，
// 前几十帧会明显乱窜。圆周起手 + 碰撞消解 = 更快进入可看的状态。
function draw() {
  NS = []; ES = []; PS = []; BY = {};
  var box = stageBox(), R = Math.min(box.w, box.h) * 0.34, i;
  DATA.nodes.forEach(function (n, idx) {
    var a = 2 * Math.PI * idx / Math.max(1, DATA.nodes.length);
    var d = {
      id: n.id, label: n.label, c: n.attribute_count || 0, p: n.parent || null,
      r: radiusOf(n.attribute_count),
      x: Math.cos(a) * R, y: Math.sin(a) * R,
      vx: 0, vy: 0, pin: false, hit: false, el: null
    };
    NS.push(d); BY[d.id] = d;
  });
  DATA.edges.forEach(function (e) {
    var a = BY[e.from], b = BY[e.to];
    if (a && b) ES.push({ a: a, b: b, label: e.label || "" });
  });
  for (i = 0; i < NS.length; i++) if (NS[i].p && BY[NS[i].p]) PS.push({ a: BY[NS[i].p], b: NS[i] });
  buildScene();
  VIEW = { k: 1, x: 0, y: 0 };
  TOUCHED = false;
  paint();
  kick(1);
}

// 点节点 → 看它的属性与说明（接口返回的 note / 属性）
function pick(id) {
  var u = "api/node?project=" + encodeURIComponent(SCOPE.project) + "&version=" + SCOPE.version + "&id=" + encodeURIComponent(id);
  ask(u)
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

say("填入口令后点「打开」；页面壳本身不含任何数据。");
</script>
</body>
</html>
"##;

/// 返回页面本体。
///
/// # ⭐ REQ-110：**没有参数、也不返回 `Result`**
///
/// 收口前签名是 `index_html(project, version) -> Result<String, String>`：
/// 它把项目名渲染进 JS 字面量、并且要过一遍 `mcp::tools::guard`。
/// 现在 `/` 是**静态壳**（壳内零数据），所以：
///
/// - 没有项目名可注入 ⇒ 没有第二个转义点（`js_string_literal` 一并删掉）；
/// - 没有数据要判 ⇒ 不需要 `Result`。模板本体仍然**运行时过闸**：
///   `index()` 把整页交给管线 H1（`Product::Text`），命中建模术语照样拒发。
///
/// ⛔ 别把项目名/版本“顺手”加回来：`GET /` 是免 token 的（ADR-004），
/// 壳里出现任何数据就等于把 `require_token` 的收窄变成一次泄露。
pub fn index_html() -> String {
    INDEX_HTML.to_string()
}
