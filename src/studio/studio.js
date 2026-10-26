/* ============================================================================
   ontok studio · 服务面页面（REQ-155 P2）
   ---------------------------------------------------------------
   数据来源（**只用既有 API 的返回形状**，一个字节都不改后端）：
     GET  /api/health                      免口令：项目 / 本体版本 / 部署模式
     GET  /api/catalog?page=&page_size=    分页到 has_more=false
     GET  /api/describe?name=              点节点时才取（详情 + 租户口径）
     POST /api/sparql                      查询台（默认 JSON-LD，含 @pending）

   三条纪律（ADR-010-A S1–S4）：
     1. 页面壳**零数据**：本文件里没有任何类名 / 关系名 / 属性名；
        预设查询也是**运行时**从 catalog 里挑一个类拼出来的。
     2. 口令只在 sessionStorage（key 见 TOKEN_KEY）：⛔ 不进 URL、⛔ 不进长期存储、
        ⛔ 不回填到 DOM 的 value 里。
     3. 401 ⇒ 清口令 + 只显示「需要口令 / 口令无效」，**一个字段都不渲染**。

   ⛔ 零外部依赖；DOM 一律 createElement / createElementNS / textContent
   （不用 HTML 字符串拼装；与 console 页面同一条 XSS 纪律）。
   ⛔ style 一律走 CSSOM（`el.style.cssText`）：CSP 的 style-src 是 'self'，
   没有 'unsafe-inline' ⇒ HTML 里的 style 属性会被浏览器忽略。
   ========================================================================== */
(function () {
  "use strict";

  /* ── 口令：只进 sessionStorage ───────────────────────────────── */
  var TOKEN_KEY = "ontok_studio_token";

  var API = {
    health: "/api/health",
    catalog: "/api/catalog",
    describe: "/api/describe",
    sparql: "/api/sparql"
  };

  /* 分页粒度（服务端上限 200） */
  var PAGE_SIZE = 200;
  var MAX_PAGES = 100;

  /* ── 极简 DOM 助手（不用字符串拼 HTML）────────────────────────── */
  function h(tag, attrs, kids) {
    var n = document.createElement(tag);
    if (attrs) Object.keys(attrs).forEach(function (k) {
      var v = attrs[k];
      if (v === null || v === undefined) return;
      if (k === "class") n.className = v;
      else if (k === "text") n.textContent = v;
      else if (k === "on") Object.keys(v).forEach(function (evt) {
        n.addEventListener(evt, v[evt]);
      });
      else if (k === "style") n.style.cssText = v;   /* CSP：CSSOM 才生效 */
      else n.setAttribute(k, v);
    });
    (kids || []).forEach(function (c) {
      if (c === null || c === undefined) return;
      n.appendChild(typeof c === "string" ? document.createTextNode(c) : c);
    });
    return n;
  }
  var SVGNS = "http://www.w3.org/2000/svg";
  function s(tag, attrs) {
    var n = document.createElementNS(SVGNS, tag);
    if (attrs) Object.keys(attrs).forEach(function (k) {
      if (attrs[k] !== null && attrs[k] !== undefined) n.setAttribute(k, attrs[k]);
    });
    return n;
  }
  function clear(node) { while (node && node.firstChild) node.removeChild(node.firstChild); }
  function byId(id) { return document.getElementById(id); }
  function setText(id, text) { var n = byId(id); if (n) n.textContent = text; }

  /* ── 页面状态 ───────────────────────────────────────────────── */
  var S = {
    token: "",
    health: null,
    items: [],
    classes: [],
    properties: [],
    relations: [],
    counts: { classes: 0, properties: 0, relations: 0 },
    byName: {},
    propsOf: {},
    relsOf: {},
    loaded: false
  };

  var ui = { focus: null, layers: { props: true, labels: true, weak: false }, q: "" };
  var view = { tx: 0, ty: 0, k: 1 };

  var nodes = [], edges = [], nodeByKey = {}, nodeEls = [], edgeEls = [];
  var drawerSeq = 0;

  /* ══════════════════════════════════════════════════════════════
     认证 / 启动 / 加载态
     ══════════════════════════════════════════════════════════════ */

  function authBadge(kind, text) {
    var b = byId("auth-badge");
    if (b) b.className = "badge" + (kind ? " " + kind : "");
    setText("auth-text", text);
  }

  function showBoot(title, sub) {
    setText("boot-title", title);
    setText("boot-sub", sub);
    showView("boot");
    var tabs = document.querySelectorAll("#tabs button");
    Array.prototype.slice.call(tabs).forEach(function (b) { b.disabled = true; });
  }

  function showAuthNeeded(message) {
    S.loaded = false;
    authBadge("", "未连接");
    setText("crumb-project", "—");
    setText("crumb-sub", "未连接");
    setText("badge-counts", "—");
    setText("rail-count", "—");
    setText("s-classes", "—");
    setText("s-rels", "—");
    setText("s-props", "—");
    clear(byId("rail-list"));
    clear(byId("legend"));
    clear(byId("cards"));
    var forget = byId("token-forget");
    if (forget) forget.hidden = true;
    showBoot("需要口令", message ||
      "本页只提供界面骨架。输入部署方给的访问口令后，才会向 /api/* 取数据。");
  }

  function showAuthInvalid() {
    try { window.sessionStorage.removeItem(TOKEN_KEY); } catch (e) {}
    S.token = "";
    authBadge("is-api", "口令无效");
    showBoot("口令无效",
      "服务返回 401（没有有效口令）。页面没有显示任何数据；口令也不会写进 URL 或长期存储。");
  }

  function showLoadError(message) {
    authBadge("is-api", "加载失败");
    showBoot("加载失败", message || "读取本体目录时出错。");
  }

  /* 保存口令 ⇒ 重新加载整页（页面壳是静态的，重载后由 sessionStorage 接着走） */
  function saveToken(token) {
    S.token = token;
    try { window.sessionStorage.setItem(TOKEN_KEY, token); } catch (e) {}
    window.location.reload();
  }

  function forgetToken() {
    try { window.sessionStorage.removeItem(TOKEN_KEY); } catch (e) {}
    S.token = "";
    window.location.reload();
  }

  /* ══════════════════════════════════════════════════════════════
     API（同源；口令只走 Authorization 头）
     ══════════════════════════════════════════════════════════════ */

  function unauthorized() {
    var e = new Error("UNAUTHORIZED");
    e.unauthorized = true;
    return e;
  }

  function apiGet(path) {
    return fetch(path, {
      method: "GET",
      headers: {
        "Authorization": "Bearer " + S.token,
        "Accept": "application/json"
      }
    }).then(readJson);
  }

  function readJson(res) {
    if (res.status === 401) throw unauthorized();
    return res.text().then(function (text) {
      if (!res.ok) {
        var msg = text;
        try {
          var v = JSON.parse(text);
          msg = (v && (v.message || v.code)) ? (v.code + "：" + v.message + (v.hint ? "（" + v.hint + "）" : "")) : text;
        } catch (e) {}
        var err = new Error("HTTP " + res.status + " — " + String(msg).slice(0, 400));
        err.status = res.status;
        throw err;
      }
      try { return JSON.parse(text); } catch (e) {
        throw new Error("响应不是 JSON（HTTP " + res.status + "）");
      }
    });
  }

  /* 目录：一页一页翻到 has_more = false（⛔ 不假设一次就够） */
  function fetchCatalog(onPage) {
    var acc = [];
    var counts = null;

    function one(page) {
      return apiGet(API.catalog + "?page=" + page + "&page_size=" + PAGE_SIZE).then(function (p) {
        if (!p || !Array.isArray(p.items)) throw new Error("catalog 响应形状不认识（缺 items）");
        acc = acc.concat(p.items);
        if (!counts) counts = p.counts || null;
        if (onPage) onPage(acc.length, page);
        if (p.has_more) {
          if (page >= MAX_PAGES) throw new Error("目录页数超过 " + MAX_PAGES + " 页，已停止（请报告）");
          return one(page + 1);
        }
        return { items: acc, counts: counts || { classes: 0, properties: 0, relations: 0 } };
      });
    }
    return one(1);
  }

  function boot() {
    var stored = "";
    try { stored = window.sessionStorage.getItem(TOKEN_KEY) || ""; } catch (e) {}
    if (!stored) { showAuthNeeded(); return; }
    S.token = stored;
    authBadge("", "连接中…");
    showBoot("加载中…", "正在读取本体目录（分页到 has_more=false）…");

    // health 免口令；失败不致命（页面仍可用目录与查询）。
    var health = apiGet(API.health).catch(function () { return null; });

    Promise.all([fetchCatalog(function (n, page) {
      setText("boot-sub", "已读取 " + n + " 条目录（第 " + page + " 页）…");
    }), health]).then(function (out) {
      S.items = out[0].items;
      S.counts = out[0].counts;
      S.health = out[1] || {};
      try { window.sessionStorage.setItem(TOKEN_KEY, stored); } catch (e) {}
      S.token = stored;
      buildModel();
      S.loaded = true;
      authBadge("is-live", "已连接");
      var forget = byId("token-forget");
      if (forget) forget.hidden = false;
      renderAll();
    }).catch(function (e) {
      if (e && e.unauthorized) { showAuthInvalid(); return; }
      showLoadError(e && e.message ? e.message : String(e));
    });
  }

  /* ══════════════════════════════════════════════════════════════
     建模：把 catalog 的平铺 items 变成图（⛔ 不写死任何名字）
     ══════════════════════════════════════════════════════════════ */

  function buildModel() {
    var items = S.items || [];
    S.classes = items.filter(function (i) { return i.kind === "class"; });
    S.properties = items.filter(function (i) { return i.kind === "property"; });
    S.relations = items.filter(function (i) { return i.kind === "relation"; });
    if (!S.counts || (!S.counts.classes && !S.counts.relations && !S.counts.properties)) {
      S.counts = {
        classes: S.classes.length,
        properties: S.properties.length,
        relations: S.relations.length
      };
    }

    S.byName = {};
    S.classes.forEach(function (c) { S.byName[c.name] = c; });

    S.propsOf = {};
    S.properties.forEach(function (p) {
      (S.propsOf[p["class"]] || (S.propsOf[p["class"]] = [])).push(p);
    });

    S.relsOf = {};
    S.relations.forEach(function (r) {
      (S.relsOf[r.from] || (S.relsOf[r.from] = [])).push(r);
      (S.relsOf[r.to] || (S.relsOf[r.to] = [])).push(r);
    });
  }

  /* ── 关系证据等级：图例按**真实分布**生成，绝不写死 ──────────── */
  var EV = {
    "true": { label: "实测", tone: "#5fd3a3", dash: false, cls: "ev-strong" },
    "inherit": { label: "继承", tone: "#6ec8ff", dash: true, cls: "ev-inherit" },
    "unverified": { label: "未验证", tone: "#e6b455", dash: true, cls: "ev-weak" },
    "false": { label: "名字推断", tone: "#ef7a8a", dash: true, cls: "ev-weak" }
  };
  function evOf(r) {
    var k = String(r.evidence_level || "").toLowerCase();
    return EV[k] || EV.unverified;
  }

  function isApi(c) { return !!((c.access || {}).kind === "api"); }

  /* 可查性三态 —— 只用现有字段推导，不新增后端语义 */
  function verifiability(c) {
    var mode = (S.health || {}).mode;
    if (isApi(c)) {
      return {
        key: "api", label: "需取数", tone: "warn",
        why: "API 源：平台不执行查询，会给 @pending 取数指引（可下推：" +
          (((c.access && c.access.pushdown_fields) || []).join(", ") || "无") + "）"
      };
    }
    if (mode === "ontology-only") {
      return {
        key: "none", label: "不可查", tone: "dim",
        why: "库源，但本部署是本体模式（不接业务库）⇒ 查询会 503 NO_DATA_SOURCE"
      };
    }
    return { key: "ok", label: "可查", tone: "ok", why: "库源，可走 SPARQL" };
  }

  function acronym(key) {
    var seg = String(key || "").split("_").filter(Boolean);
    if (!seg.length) return "?";
    if (seg[0].length <= 4) return seg[0].toUpperCase();
    if (seg.length > 1) return seg.map(function (x) { return x[0]; }).join("").toUpperCase().slice(0, 3);
    return seg[0].slice(0, 3).toUpperCase();
  }

  function shortName(name) {
    var parts = String(name || "").split("__");
    return parts.length > 1 ? parts[1] : String(name || "");
  }

  function iri(name) {
    var n = String(name || "");
    return n.charAt(0) === ":" ? n : ":" + n;
  }

  /* ══════════════════════════════════════════════════════════════
     图：布局（确定性初始角）+ 力导向收敛
     ══════════════════════════════════════════════════════════════ */

  var svg = byId("graph");
  var gRoot = s("g", { id: "g-root" });
  var gEdges = s("g", { "class": "g-edges" });
  var gNodes = s("g");
  var gProps = s("g", { opacity: "0" });

  function buildGraph() {
    clear(svg);
    nodeEls = [];
    edgeEls = [];
    nodeByKey = {};
    nodes = S.classes.map(function (c, i) {
      var a = (i / Math.max(1, S.classes.length)) * Math.PI * 2;
      var r = 300 + (i % 3) * 34;
      return { key: c.name, c: c, x: Math.cos(a) * r, y: Math.sin(a) * r, vx: 0, vy: 0, deg: 0 };
    });
    nodes.forEach(function (n) { nodeByKey[n.key] = n; });
    edges = S.relations.map(function (r) {
      return { r: r, a: nodeByKey[r.from], b: nodeByKey[r.to] };
    }).filter(function (e) { return e.a && e.b; });
    edges.forEach(function (e) { e.a.deg++; e.b.deg++; });

    gRoot = s("g", { id: "g-root" });
    gEdges = s("g", { "class": "g-edges" });
    gNodes = s("g");
    gProps = s("g", { opacity: "0" });
    gRoot.appendChild(gEdges);
    gRoot.appendChild(gProps);
    gRoot.appendChild(gNodes);
    svg.appendChild(gRoot);

    simulate();
    placeLabels();
    injectDefs();
    buildEdgeEls();
    buildNodeEls();
  }

  function simulate() {
    var K = 8000, SPRING = 0.012, REST = 280, GRAV = 0.0008, DAMP = 0.86;
    for (var it = 0; it < 800; it++) {
      for (var i = 0; i < nodes.length; i++) {
        for (var j = i + 1; j < nodes.length; j++) {
          var a = nodes[i], b = nodes[j];
          var dx = b.x - a.x, dy = b.y - a.y;
          var d2 = dx * dx + dy * dy || 0.01;
          var f = K / d2;
          var d = Math.sqrt(d2);
          var ux = dx / d, uy = dy / d;
          a.vx -= ux * f; a.vy -= uy * f;
          b.vx += ux * f; b.vy += uy * f;
        }
      }
      edges.forEach(function (e) {
        var dx = e.b.x - e.a.x, dy = e.b.y - e.a.y;
        var d = Math.sqrt(dx * dx + dy * dy) || 0.01;
        var f = (d - REST) * SPRING;
        var ux = dx / d, uy = dy / d;
        e.a.vx += ux * f; e.a.vy += uy * f;
        e.b.vx -= ux * f; e.b.vy -= uy * f;
      });
      nodes.forEach(function (n) {
        n.vx -= n.x * GRAV; n.vy -= n.y * GRAV;
        n.vx *= DAMP; n.vy *= DAMP;
        n.x += n.vx; n.y += n.vy;
      });
    }
  }

  /* 标签避让：同屏 30 个类，长中文名会互相压（贪心：按 y 排序、纵向下挪） */
  function placeLabels() {
    var maxChars = 8;
    var placed = [];
    nodes.slice().sort(function (a, b) { return a.y - b.y; }).forEach(function (n) {
      var full = n.c.display_name || n.key;
      n.labelText = full.length > maxChars ? full.slice(0, maxChars - 1) + "…" : full;
      var w = n.labelText.length * 12.4;
      var dy = 0;
      for (var guard = 0; guard < 24; guard++) {
        var rect = { x0: n.x - w / 2 - 2, x1: n.x + w / 2 + 22, y0: n.y + 32 + dy, y1: n.y + 68 + dy };
        var hit = placed.some(function (r) {
          return !(rect.x1 < r.x0 || rect.x0 > r.x1 || rect.y1 < r.y0 || rect.y0 > r.y1);
        });
        if (!hit) { placed.push(rect); break; }
        dy += 13;
      }
      n.labelDy = dy;
    });
  }

  /* defs：立体球用的渐变（曾用 SVG 滤镜做泛光，会在每帧重算导致闪烁，⛔ 不要加回来） */
  function injectDefs() {
    var defs = s("defs", {});
    function radialGrad(id, stops) {
      var g = s("radialGradient", { id: id, cx: "34%", cy: "27%", r: "80%" });
      stops.forEach(function (st) {
        g.appendChild(s("stop", {
          offset: st[0] + "%", "stop-color": st[1],
          "stop-opacity": st[2] === undefined ? "1" : String(st[2])
        }));
      });
      defs.appendChild(g);
    }
    radialGrad("sphere-warn", [[0, "#ffecc0", 0.95], [26, "#e2b45c", 0.9], [62, "#7d5a18", 0.85], [100, "#150f04", 0.92]]);
    radialGrad("sphere-dim", [[0, "#e6eefa", 0.9], [26, "#7d8ca6", 0.85], [62, "#39465c", 0.8], [100, "#0b111b", 0.9]]);
    radialGrad("sphere-ok", [[0, "#e3f6ff", 0.95], [26, "#5fa3cd", 0.9], [62, "#1d526f", 0.85], [100, "#06161f", 0.92]]);
    radialGrad("glow-warn", [[0, "#e6b455", 0.55], [55, "#e6b455", 0.16], [100, "#e6b455", 0]]);
    radialGrad("glow-dim", [[0, "#7f8ea8", 0.4], [55, "#7f8ea8", 0.12], [100, "#7f8ea8", 0]]);
    radialGrad("glow-ok", [[0, "#6ec8ff", 0.55], [55, "#6ec8ff", 0.16], [100, "#6ec8ff", 0]]);
    svg.appendChild(defs);
  }

  function edgeTone(r) { return evOf(r).tone; }
  function isWeak(r) { return evOf(r).dash; }

  function buildEdgeEls() {
    edgeEls = [];
    edges.forEach(function (e, i) {
      var line = s("path", {
        "class": "edge-line",
        stroke: edgeTone(e.r),
        "stroke-width": String(0.9 + (String(e.r.evidence_level).toLowerCase() === "true" ? 0.5 : 0)),
        "stroke-dasharray": isWeak(e.r) ? "4 4" : null
      });
      line.style.animationDelay = (i * 9) + "ms";
      var lbl = s("text", { "class": "edge-label" });
      /* ⚠️ 这里曾经用 shortName(名字) —— 但本套 34 条关系的名字都是 `from__to`，
         shortName 取的是**后半段（目标类名）**，看起来像"关系没有名字"。
         开关既然叫「关系名」，就显示真名。 */
      var fullName = String(e.r.name || "");
      lbl.textContent = fullName.length > 18 ? fullName.slice(0, 17) + "…" : fullName;
      if (fullName) lbl.setAttribute("title", fullName);
      var lbg = s("rect", { "class": "edge-label-bg", rx: "3", height: "13" });
      gEdges.appendChild(line);
      gEdges.appendChild(lbg);
      gEdges.appendChild(lbl);
      edgeEls.push({ e: e, line: line, lbl: lbl, lbg: lbg });
    });
  }

  /* 边与标签的几何：**必须真的算 `d`**（漏了这一步 ⇒ 一条线都不显示） */
  function syncEdges() {
    var placed = [];
    edgeEls.forEach(function (o) {
      var a = o.e.a, b = o.e.b;
      var dx = b.x - a.x, dy = b.y - a.y;
      var len = Math.sqrt(dx * dx + dy * dy) || 1;
      var pad = 27;
      var x1 = a.x + dx / len * pad, y1 = a.y + dy / len * pad;
      var x2 = b.x - dx / len * pad, y2 = b.y - dy / len * pad;
      o.line.setAttribute("d", "M" + x1 + "," + y1 + "L" + x2 + "," + y2);

      var mx = (x1 + x2) / 2, my = (y1 + y2) / 2;
      var txt = o.lbl.textContent || "";
      var w = Math.max(12, txt.length * 6.2 + 8);
      o.lbl.setAttribute("x", mx);
      o.lbl.setAttribute("y", my - 6);
      o.lbl.setAttribute("text-anchor", "middle");
      o.lbg.setAttribute("x", mx - w / 2);
      o.lbg.setAttribute("y", my - 15);
      o.lbg.setAttribute("width", w);

      /* 描边样式 + 透明度：**这里唯一决定**（focusNode 不再写） */
      var weak = isWeak(o.e.r);
      var near = !ui.focus || o.e.a.key === ui.focus || o.e.b.key === ui.focus;
      o.line.setAttribute("stroke-dasharray", weak ? "4 4" : "");
      /* 「弱证据」只改**弱边的强调程度**，不决定它存不存在：
         本套 34 条关系全是 unverified，若让它决定显隐，默认一条边都看不见。 */
      o.line.setAttribute("opacity",
        !near ? "0.06" : (weak ? (ui.layers.weak ? "0.95" : "0.55") : "0.85"));

      /* 关系名：只看 labels + 焦点 + 放不放得下（⛔ 不再被「弱证据」门控） */
      var show = ui.layers.labels && near && len > 74;
      if (show) {
        var rect = { x0: mx - w / 2 - 1, x1: mx + w / 2 + 1, y0: my - 15, y1: my - 2 };
        for (var i = 0; i < placed.length; i++) {
          var r = placed[i];
          if (!(rect.x1 < r.x0 || rect.x0 > r.x1 || rect.y1 < r.y0 || rect.y0 > r.y1)) { show = false; break; }
        }
        if (show) placed.push(rect);
      }
      o.lbl.setAttribute("opacity", show ? (ui.focus ? "0.95" : "0.78") : "0");
      o.lbg.setAttribute("opacity", show ? (ui.focus ? "0.9" : "0.75") : "0");
    });
  }

  function buildNodeEls() {
    nodeEls = [];
    nodes.forEach(function (n) {
      var v = verifiability(n.c);
      var tone = v.key === "api" ? "#e6b455" : v.key === "ok" ? "#6ec8ff" : "#5d6a80";
      var kind = v.key === "api" ? "warn" : v.key === "ok" ? "ok" : "dim";
      var g = s("g", { "class": "node-g", "data-key": n.key });
      n.g = g;

      var halo = s("circle", {
        "class": "node-halo", cx: n.x, cy: n.y,
        r: 42 + Math.min(16, n.deg * 2.6), fill: "url(#glow-" + kind + ")"
      });
      var orbit = s("circle", {
        "class": "node-orbit", cx: n.x, cy: n.y, r: 28.5,
        stroke: tone, "stroke-dasharray": "2 7", "stroke-width": "1.1"
      });
      var sphere = s("circle", {
        "class": "node-sphere", cx: n.x, cy: n.y, r: 21.5,
        fill: "url(#sphere-" + kind + ")"
      });
      var rim = s("circle", {
        "class": "node-rim", cx: n.x, cy: n.y, r: 21.5,
        fill: "none", stroke: tone, "stroke-width": "1.1"
      });
      var spec = s("ellipse", {
        "class": "node-spec", cx: n.x - 6.6, cy: n.y - 7.4,
        rx: 6, ry: 4.2, fill: "#fff", opacity: "0.18"
      });
      var ring = s("circle", { "class": "node-ring", cx: n.x, cy: n.y, r: 25.6, stroke: tone });

      var core = s("text", {
        "class": "node-core", x: n.x, y: n.y + 4.2, "text-anchor": "middle",
        fill: "#0b0f16", "font-size": "10.5", "font-weight": "700",
        "font-family": "ui-monospace, monospace", "letter-spacing": "0.3",
        "pointer-events": "none", opacity: "0.82"
      });
      core.textContent = acronym(n.key);

      var label = s("text", { "class": "node-label", x: n.x, y: n.y + 42 + (n.labelDy || 0) });
      label.textContent = n.labelText || n.c.display_name || n.key;
      var sub = s("text", { "class": "node-sub", x: n.x, y: n.y + 54 + (n.labelDy || 0) });
      sub.textContent = (n.c.property_count || 0) + " 属性";

      g.appendChild(halo); g.appendChild(orbit); g.appendChild(sphere);
      g.appendChild(rim); g.appendChild(spec); g.appendChild(ring);
      g.appendChild(core); g.appendChild(label); g.appendChild(sub);
      gNodes.appendChild(g);

      g.style.animationDelay = (nodeEls.length * 14) + "ms";
      g.addEventListener("pointerdown", function (ev) { startDrag(ev, n); });
      g.addEventListener("click", function (ev) { ev.stopPropagation(); if (!dragMoved) focusNode(n.key); });
      g.addEventListener("mouseenter", function (ev) { showTip(ev, n); });
      g.addEventListener("mousemove", moveTip);
      g.addEventListener("mouseleave", hideTip);

      nodeEls.push({
        n: n, g: g, halo: halo, orbit: orbit, sphere: sphere,
        rim: rim, spec: spec, ring: ring, core: core, label: label, sub: sub, tone: tone
      });
    });
  }

  function syncNodes() {
    nodeEls.forEach(function (o) {
      var n = o.n, dy = n.labelDy || 0;
      o.halo.setAttribute("cx", n.x); o.halo.setAttribute("cy", n.y);
      o.orbit.setAttribute("cx", n.x); o.orbit.setAttribute("cy", n.y);
      o.sphere.setAttribute("cx", n.x); o.sphere.setAttribute("cy", n.y);
      o.rim.setAttribute("cx", n.x); o.rim.setAttribute("cy", n.y);
      o.spec.setAttribute("cx", n.x - 6.6); o.spec.setAttribute("cy", n.y - 7.4);
      o.ring.setAttribute("cx", n.x); o.ring.setAttribute("cy", n.y);
      o.core.setAttribute("x", n.x); o.core.setAttribute("y", n.y + 4);
      o.label.setAttribute("x", n.x); o.label.setAttribute("y", n.y + 40 + dy);
      o.sub.setAttribute("x", n.x); o.sub.setAttribute("y", n.y + 52 + dy);
    });
  }

  /* ══════════════════════════════════════════════════════════════
     物理引擎：斥力 + 弹簧 + 重力 + 碰撞 + 阻尼（退火保证会停）
     ══════════════════════════════════════════════════════════════ */
  var REST_LEN = 280;
  var MIN_GAP = 88;
  var dragNode = null, dragMoved = false, dragging = false, lastX = 0, lastY = 0;
  var physics = { running: false, alpha: 1 };

  function applyForces(al) {
    var i, j, a, b, dx, dy, d2, d, f, ux, uy;
    nodes.forEach(function (n) { n.fx = 0; n.fy = 0; });
    for (i = 0; i < nodes.length; i++) {
      for (j = i + 1; j < nodes.length; j++) {
        a = nodes[i]; b = nodes[j];
        dx = b.x - a.x; dy = b.y - a.y;
        d2 = dx * dx + dy * dy;
        if (d2 < 1) { d2 = 1; dx = (i % 2 ? 0.6 : -0.6); dy = (j % 2 ? 0.6 : -0.6); }
        d = Math.sqrt(d2);
        f = 8000 / d2 * al;
        ux = dx / d; uy = dy / d;
        a.fx -= ux * f; a.fy -= uy * f;
        b.fx += ux * f; b.fy += uy * f;
      }
    }
    edges.forEach(function (e) {
      dx = e.b.x - e.a.x; dy = e.b.y - e.a.y;
      d = Math.sqrt(dx * dx + dy * dy) || 1;
      f = (d - REST_LEN) * 0.016 * al;
      ux = dx / d; uy = dy / d;
      e.a.fx += ux * f; e.a.fy += uy * f;
      e.b.fx -= ux * f; e.b.fy -= uy * f;
    });
    nodes.forEach(function (n) { n.fx -= n.x * 0.0016 * al; n.fy -= n.y * 0.0016 * al; });
  }

  function integrate() {
    nodes.forEach(function (n) {
      if (n.pinned) { n.vx = 0; n.vy = 0; return; }
      n.vx = (n.vx + n.fx) * 0.72;
      n.vy = (n.vy + n.fy) * 0.72;
      n.x += n.vx; n.y += n.vy;
    });
    for (var i = 0; i < nodes.length; i++) {
      for (var j = i + 1; j < nodes.length; j++) {
        var a = nodes[i], b = nodes[j];
        var dx = b.x - a.x, dy = b.y - a.y;
        var d = Math.sqrt(dx * dx + dy * dy);
        if (d > 0.0001 && d < MIN_GAP) {
          var push = (MIN_GAP - d) / 2 * 0.9;
          var ux = dx / d, uy = dy / d;
          if (!a.pinned) { a.x -= ux * push; a.y -= uy * push; }
          if (!b.pinned) { b.x += ux * push; b.y += uy * push; }
        }
      }
    }
  }

  function tick() {
    physics.alpha = dragNode ? Math.max(physics.alpha * 0.94, 0.5) : physics.alpha * 0.94;
    applyForces(physics.alpha);
    integrate();
    syncNodes(); syncEdges(); renderProps();
    if (!dragNode && physics.alpha < 0.03) { physics.running = false; return; }
    window.requestAnimationFrame(tick);
  }
  function kick() {
    if (!physics.running) {
      physics.running = true;
      /* ⚠️ 不要把 alpha 重置成 1：那会让整张图重新收敛（松手后"闪一下"） */
      physics.alpha = Math.max(physics.alpha, 0.5);
      window.requestAnimationFrame(tick);
    }
  }

  /* ══════════════════════════════════════════════════════════════
     视图 / 抽屉 / 侧栏 / 图例 / 卡片
     ══════════════════════════════════════════════════════════════ */

  var views = { boot: "v-boot", graph: "v-graph", catalog: "v-catalog", query: "v-query" };
  function showView(name) {
    Object.keys(views).forEach(function (k) {
      var el = byId(views[k]);
      if (el) el.classList.toggle("is-on", k === name);
    });
    Array.prototype.slice.call(document.querySelectorAll("#tabs button")).forEach(function (b) {
      b.classList.toggle("is-on", b.getAttribute("data-view") === name);
    });
    if (name !== "graph") closeDrawer();
    if (name === "graph" && S.loaded) applyView();
  }

  function renderProps() {
    clear(gProps);
    /* ⭐ 开关必须在这里生效：之前只有点击时才 clear，一聚焦又画回来 */
    if (!ui.layers.props) { gProps.setAttribute("opacity", "0"); return; }
    if (!ui.focus) { gProps.setAttribute("opacity", "0"); return; }
    var n = nodeByKey[ui.focus];
    var list = (S.propsOf[ui.focus] || []).slice(0, 8);
    if (!n || !list.length) { gProps.setAttribute("opacity", "0"); return; }
    var R = 96;
    list.forEach(function (p, i) {
      var a = (i / list.length) * Math.PI * 2 - Math.PI / 2;
      var x = n.x + Math.cos(a) * R, y = n.y + Math.sin(a) * R;
      var dot = s("circle", { cx: x, cy: y, r: 3.2, fill: "#6ec8ff", opacity: "0.5" });
      var t = s("text", {
        x: x, y: y - 8, "text-anchor": "middle", fill: "#8fa0bb",
        "font-size": "9.5", "font-family": "ui-monospace, monospace"
      });
      t.textContent = shortName(p.name);
      gProps.appendChild(dot); gProps.appendChild(t);
    });
    gProps.setAttribute("opacity", "1");
  }

  /* 图层开关的**唯一应用点**：props / labels / weak 都在这里落地 */
  function applyLayers() {
    renderProps();
    syncEdges();
  }

  function focusNode(key) {
    ui.focus = key;
    var near = {};
    if (key) {
      near[key] = 1;
      edges.forEach(function (e) {
        if (e.a.key === key) near[e.b.key] = 1;
        if (e.b.key === key) near[e.a.key] = 1;
      });
    }
    nodeEls.forEach(function (o) {
      var k = o.n.key;
      o.g.classList.toggle("is-focus", ui.focus === k);
      o.g.classList.toggle("is-near", !!ui.focus && ui.focus !== k && !!near[k]);
      o.g.classList.toggle("is-faded", !!ui.focus && !near[k]);
    });
    syncEdges();   /* 边的描边/透明度只在这里决定（单一写者） */
    renderProps();
    if (key) openDrawer(key); else closeDrawer();
    highlightRail(key);
  }

  var drawer = byId("drawer");

  function openDrawer(key) {
    document.body.classList.add("drawer-open");
    var seq = ++drawerSeq;
    renderDrawer(key, null);
    apiGet(API.describe + "?name=" + encodeURIComponent(iri(key))).then(function (d) {
      if (seq !== drawerSeq || ui.focus !== key) return;
      renderDrawer(key, d && d.kind === "class" ? d : null);
    }).catch(function () { /* 详情取不到就保留目录事实，⛔ 不编数据 */ });
  }
  function closeDrawer() {
    if (drawer) drawer.classList.remove("is-open");
    document.body.classList.remove("drawer-open");
  }

  function renderDrawer(key, detail) {
    var c = S.byName[key];
    if (!c) return;
    var v = verifiability(c);
    setText("d-title", c.display_name || key);
    setText("d-key", iri(key));
    var body = byId("d-body");
    clear(body);

    /* 概况 */
    var kv = [
      h("dt", { text: "可查性" }),
      h("dd", {}, [h("span", { class: "pill " + (isApi(c) ? "api" : "db"), text: v.label })]),
      h("dt", { text: "说明" }), h("dd", { text: v.why }),
      h("dt", { text: "规模" }), h("dd", {
        text: (c.property_count || 0) + " 属性 · " + (c.relation_count || 0) + " 关系"
      })
    ];
    if (detail) {
      if (detail.table) { kv.push(h("dt", { text: "对象" })); kv.push(h("dd", { text: detail.table })); }
      if (detail.tenant && detail.tenant.mode) {
        kv.push(h("dt", { text: "租户口径" }));
        kv.push(h("dd", { text: detail.tenant.mode + (detail.tenant.column ? "（列 " + detail.tenant.column + "）" : "") }));
      }
      if (detail.display_name && detail.display_name !== key) {
        kv.push(h("dt", { text: "业务名" })); kv.push(h("dd", { text: detail.display_name }));
      }
    }
    if (c.description) { kv.push(h("dt", { text: "定义" })); kv.push(h("dd", { text: c.description })); }
    body.appendChild(h("div", { class: "sect" }, [
      h("div", { class: "sect-h", text: detail ? "概况（/api/describe）" : "概况（/api/catalog）" }),
      h("dl", { class: "kv" }, kv)
    ]));

    /* 属性：优先用 describe 的（带类型 / 枚举 / 样例），否则用目录里的 */
    var props = (detail && detail.properties && detail.properties.length)
      ? detail.properties
      : (S.propsOf[key] || []);
    if (props.length) {
      body.appendChild(h("div", { class: "sect" }, [
        h("div", { class: "sect-h", text: "属性 · " + props.length }),
        h("div", { class: "prop-list" }, props.map(function (p) {
          var kids = [
            h("span", { class: "pn", text: p.display_name || shortName(p.name) }),
            h("span", { class: "pk", text: shortName(p.name) })
          ];
          if (p.datatype) kids.push(h("span", { class: "pt", text: String(p.datatype) }));
          if (p.enum_values) kids.push(h("span", { class: "pt", text: "枚举" }));
          return h("div", { class: "prop", title: p.enum_values ? String(p.enum_values) : null }, kids);
        }))
      ]));
    }

    /* 关系（带证据等级） */
    var rels = (detail && detail.relations && detail.relations.length)
      ? detail.relations
      : (S.relsOf[key] || []);
    if (rels.length) {
      body.appendChild(h("div", { class: "sect" }, [
        h("div", { class: "sect-h", text: "关系 · " + rels.length }),
        h("div", { class: "rel-list" }, rels.map(function (r) {
          var ev = evOf(r);
          var other = r.from === key ? r.to : r.from;
          var dir = r.from === key ? "→" : "←";
          /* ⚠️ 曾经显示 shortName(name)：34 条关系名都是 `from__to`，shortName 取后半段
                   （目标类名），看起来像"关系没有名字"。现在给：方向 + 对方类 + 完整名字 + 证据。 */
                return h("div", { class: "rel" }, [
                  h("span", { class: "arrow", text: dir }),
                  h("b", { text: other }),
                  h("span", { class: "pk", text: String(r.name || "") }),
                  h("span", { class: "ev " + ev.cls, text: ev.label })
                ]);
        }))
      ]));
    }

    drawer.classList.add("is-open");
  }

  /* 图例：按真实数据生成（外加"全部未验证"的诚实提示） */
  function buildLegend() {
    /* 诚实提示：本套 34 条关系的名字都是 `from__to` 自动生成的端点对，本体没有语义动词。
       ⛔ 别在 UI 里"编"一个好看的动词（那就是造假）。 */
    /* 诚实提示：关系名是 from__to 自动生成，本体没有语义动词 */
    var box = byId("legend");
    clear(box);
    var apiN = S.classes.filter(isApi).length;
    var evCount = {};
    S.relations.forEach(function (r) {
      var k = String(r.evidence_level || "").toLowerCase() || "unknown";
      evCount[k] = (evCount[k] || 0) + 1;
    });
    var rows = [];
    rows.push(h("div", { class: "legend-row" }, [
      h("span", { class: "legend-swatch", style: "color:#e6b455;background:#e6b455" }),
      h("span", { text: "API 源 " + apiN + " 类 · 平台不执行，按 get 自取" })
    ]));
    rows.push(h("div", { class: "legend-row" }, [
      h("span", { class: "legend-swatch", style: "color:#5d6a80;background:#5d6a80" }),
      h("span", { text: "库源 " + (S.classes.length - apiN) + " 类" })
    ]));
    Object.keys(evCount).forEach(function (k) {
      var ev = EV[k] || EV.unverified;
      rows.push(h("div", { class: "legend-row" }, [
        h("span", {
          class: "legend-line",
          style: "border-color:" + ev.tone + (ev.dash ? ";border-top-style:dashed" : "")
        }),
        h("span", { text: "关系 " + ev.label + " · " + evCount[k] + " 条" })
      ]));
    });
    var allUnverified = Object.keys(evCount).length === 1 &&
      evCount.unverified === S.relations.length && S.relations.length > 0;
    if (allUnverified) {
      rows.push(h("div", {
        class: "legend-row",
        style: "color:#e6b455;align-items:flex-start;line-height:1.45;margin-top:2px"
      }, [h("span", {
        text: "⚠️ 全部 " + S.relations.length + " 条关系都是"
          + "文档推断、无实测证据（evidence 为空）—— 当作语义提示，不要当已验证的事实。"
      })]));
    }
    rows.forEach(function (r) { box.appendChild(r); });
  }

  /* 左侧栏 */
  var railItems = {};
  function buildRail() {
    var railList = byId("rail-list");
    clear(railList);
    railItems = {};
    S.classes.slice().sort(function (a, b) {
      return String(a.display_name || a.name).localeCompare(String(b.display_name || b.name), "zh");
    }).forEach(function (c) {
      var v = verifiability(c);
      var row = h("div", { class: "rail-item", "data-key": c.name }, [
        h("div", { class: "ico", text: acronym(c.name).slice(0, 2) }),
        h("div", { class: "nm" }, [
          h("b", { text: c.display_name || c.name }),
          h("i", { text: c.name })
        ]),
        v.key === "api" ? h("span", { class: "tag api", text: "API" }) : null
      ]);
      row.addEventListener("click", function () { showView("graph"); focusNode(c.name); });
      railList.appendChild(row);
      railItems[c.name] = row;
    });
  }

  function highlightRail(key) {
    Object.keys(railItems).forEach(function (k) {
      railItems[k].classList.toggle("is-active", k === key);
      railItems[k].classList.remove("is-dim");
    });
    if (!key) return;
    Object.keys(railItems).forEach(function (k) {
      var near = k === key || edges.some(function (e) {
        return (e.a.key === key && e.b.key === k) || (e.b.key === key && e.a.key === k);
      });
      if (!near) railItems[k].classList.add("is-dim");
    });
  }

  /* 目录卡片 */
  var cardEls = {};
  function buildCards() {
    var cards = byId("cards");
    clear(cards);
    cardEls = {};
    S.classes.forEach(function (c) {
      var v = verifiability(c);
      var card = h("div", { class: "card", "data-key": c.name }, [
        h("div", { class: "card-top" }, [
          h("div", { class: "card-name", text: c.display_name || c.name }),
          h("span", { class: "pill " + (isApi(c) ? "api" : "db"), text: v.label })
        ]),
        h("div", { class: "card-key", text: iri(c.name) }),
        h("div", { class: "card-meta" }, [
          h("span", {}, [h("b", { text: String(c.property_count || 0) }), " 属性"]),
          h("span", {}, [h("b", { text: String(c.relation_count || 0) }), " 关系"])
        ])
      ]);
      card.addEventListener("click", function () { showView("graph"); focusNode(c.name); });
      cards.appendChild(card);
      cardEls[c.name] = card;
    });
    setText("catalog-sub", S.classes.length + " 个类 · 其中 " +
      S.classes.filter(isApi).length + " 个来自 API 源（平台不执行，给取数指引）");
  }

  /* ══════════════════════════════════════════════════════════════
     查询台（POST /api/sparql；回执默认 JSON-LD）
     ══════════════════════════════════════════════════════════════ */
  var qText = byId("q-text");
  var qOut = byId("q-out");
  var qStatus = byId("q-status");

  function presets() {
    var api = S.classes.filter(isApi)[0];
    var db = S.classes.filter(function (c) { return !isApi(c); })[0];
    var list = [];
    if (api) {
      list.push({
        label: "API 源类 · 应给 @pending",
        q: "SELECT ?s WHERE { ?s a " + iri(api.name) + " } LIMIT 3"
      });
    }
    if (db) {
      list.push({
        label: "库源类 · 看本部署能否执行",
        q: "SELECT ?s WHERE { ?s a " + iri(db.name) + " } LIMIT 3"
      });
    }
    return list;
  }

  function buildSamples() {
    var bar = byId("q-samples");
    clear(bar);
    var list = presets();
    list.forEach(function (p) {
      var b = h("button", { class: "q-sample", text: p.label });
      b.addEventListener("click", function () {
        qText.value = p.q;
        runQuery();
      });
      bar.appendChild(b);
    });
    if (list.length) qText.value = list[0].q;
  }

  function chip(status) {
    var cls = status === 200 ? "chip-200" : status === 400 ? "chip-400" : "chip-503";
    return h("span", { class: "chip-code " + cls, text: "HTTP " + status });
  }

  function renderJson(v) {
    var frag = document.createDocumentFragment();
    function walk(x, depth, pad) {
      var ind = new Array(pad + 1).join("  ");
      if (x === null) { frag.appendChild(h("span", { class: "json-null", text: "null" })); return; }
      if (Array.isArray(x)) {
        frag.appendChild(document.createTextNode("["));
        if (!x.length) { frag.appendChild(document.createTextNode("]")); return; }
        frag.appendChild(document.createTextNode("\n"));
        x.forEach(function (it, i) {
          frag.appendChild(document.createTextNode(ind + "  "));
          walk(it, depth + 1, pad + 1);
          frag.appendChild(document.createTextNode(i < x.length - 1 ? ",\n" : "\n"));
        });
        frag.appendChild(document.createTextNode(ind + "]"));
        return;
      }
      if (typeof x === "object") {
        frag.appendChild(document.createTextNode("{"));
        var ks = Object.keys(x);
        if (!ks.length) { frag.appendChild(document.createTextNode("}")); return; }
        frag.appendChild(document.createTextNode("\n"));
        ks.forEach(function (k, i) {
          frag.appendChild(document.createTextNode(ind + "  "));
          frag.appendChild(h("span", { class: "json-key", text: JSON.stringify(k) }));
          frag.appendChild(document.createTextNode(": "));
          walk(x[k], depth + 1, pad + 1);
          frag.appendChild(document.createTextNode(i < ks.length - 1 ? ",\n" : "\n"));
        });
        frag.appendChild(document.createTextNode(ind + "}"));
        return;
      }
      if (typeof x === "string") frag.appendChild(h("span", { class: "json-str", text: JSON.stringify(x) }));
      else if (typeof x === "number") frag.appendChild(h("span", { class: "json-num", text: String(x) }));
      else if (typeof x === "boolean") frag.appendChild(h("span", { class: "json-bool", text: String(x) }));
      else frag.appendChild(document.createTextNode(String(x)));
    }
    walk(v, 0, 0);
    return frag;
  }

  /* @pending：本产品最容易被误读的地方 —— 必须说清"没给数据，给的是取数指引" */
  function pendingBox(pending, meta) {
    var box = h("div", { class: "pending-box" });
    box.appendChild(h("div", {}, [h("b", { text: "这次查询没给数据，给的是取数指引（@pending）" })]));
    pending.slice(0, 5).forEach(function (p) {
      var get = p.get || {};
      var cls = String(p["@type"] || get["class"] || "").split(/[\/#]/).pop();
      var line = "类 " + (get["class"] || cls || "?") + " · reason " + (p.reason || "?");
      box.appendChild(h("div", { class: "pending-line" }, [h("span", { text: line })]));
      var ids = ((get.filter || {}).ids) || [];
      if (ids.length) {
        box.appendChild(h("div", { class: "pending-line" }, [h("span", { text: "filter.ids " + ids.join("、") })]));
      }
      if (get.pushdown && get.pushdown.length) {
        box.appendChild(h("div", { class: "pending-line" }, [
          h("span", { text: "可下推 " }),
          h("code", { text: get.pushdown.join(" / ") })
        ]));
      }
      if (get.ref) {
        box.appendChild(h("div", { class: "pending-line" }, [
          h("span", { text: "按 " }),
          h("code", { text: String(get.ref) }),
          h("span", { text: " 取数（同一份映射契约）" })
        ]));
      }
      if (p.hint) {
        box.appendChild(h("div", { class: "pending-line" }, [h("span", { text: String(p.hint) })]));
      }
    });
    if (pending.length > 5) {
      box.appendChild(h("div", { class: "pending-line" }, [
        h("span", { text: "（还有 " + (pending.length - 5) + " 条，见下方原始回执）" })
      ]));
    }
    box.appendChild(h("div", { class: "pending-line" }, [
      h("span", { text: "⛔ filter 为空时不要全量拉 —— 先用 " }),
      h("code", { text: "describe" }),
      h("span", { text: " 找到标识属性，再带 ids 取。" })
    ]));
    var complete = (meta || {}).complete;
    box.appendChild(h("div", { class: "pending-line" }, [
      h("span", { text: "meta.complete = " }),
      h("code", { text: String(complete) }),
      h("span", { text: complete === false ? " ⇒ 这份结果不完整，不能直接当答案。" : "" })
    ]));
    return box;
  }

  function setStatus(nodes) {
    clear(qStatus);
    (nodes || []).forEach(function (n) { qStatus.appendChild(n); });
  }

  function renderResponse(status, contentType, text, elapsedMs) {
    clear(qOut);
    var body = null;
    try { body = JSON.parse(text); } catch (e) { body = null; }

    if (status === 401) { showAuthInvalid(); return; }

    if (body && body["@pending"] && body["@pending"].length) {
      qOut.appendChild(pendingBox(body["@pending"], body.meta));
    }
    if (body) {
      qOut.appendChild(h("div", { style: "white-space:pre" }, [renderJson(body)]));
    } else {
      qOut.appendChild(h("div", { text: text }));
    }

    var nodes = [chip(status)];
    nodes.push(h("span", { text: contentType || "" }));
    if (elapsedMs !== undefined) nodes.push(h("span", { text: elapsedMs + " ms" }));
    if (body && body.meta && body.meta.truncated) nodes.push(h("span", { text: "truncated" }));
    setStatus(nodes);
  }

  function runQuery() {
    var text = String(qText.value || "").trim();
    if (!text) { setStatus([h("span", { text: "空查询" })]); return; }
    setStatus([h("span", { text: "执行中…" })]);
    clear(qOut);
    var started = Date.now();
    fetch(API.sparql, {
      method: "POST",
      headers: {
        "Authorization": "Bearer " + S.token,
        "Content-Type": "application/json",
        "Accept": "application/ld+json"
      },
      body: JSON.stringify({ query: text })
    }).then(function (res) {
      return res.text().then(function (t) {
        renderResponse(res.status, res.headers.get("content-type") || "",
          t, Date.now() - started);
      });
    }).catch(function (e) {
      clear(qOut);
      qOut.appendChild(h("div", { text: "请求失败：" + (e && e.message ? e.message : String(e)) }));
      setStatus([h("span", { text: "请求失败" })]);
    });
  }

  /* ══════════════════════════════════════════════════════════════
     平移 / 缩放 / 拖拽 / 搜索 / 提示
     ══════════════════════════════════════════════════════════════ */

  function applyView() {
    if (!gRoot) return;
    gRoot.setAttribute("transform",
      "translate(" + view.tx + "," + view.ty + ") scale(" + view.k + ")");
  }

  function toGraph(ev) {
    var rect = svg.getBoundingClientRect();
    return {
      x: (ev.clientX - rect.left - view.tx) / view.k,
      y: (ev.clientY - rect.top - view.ty) / view.k
    };
  }

  function startDrag(ev, n) {
    if (ev.button !== undefined && ev.button !== 0 && ev.pointerType === "mouse") return;
    ev.stopPropagation();
    ev.preventDefault();
    dragNode = n;
    dragMoved = false;
    n.pinned = true;
    n._pid = ev.pointerId;
    svg.classList.add("is-panning");
    document.body.style.cursor = "grabbing";
    try { if (n.g) n.g.setAttribute("class", "node-g is-dragging"); } catch (e) {}
    try { if (n.g) n.g.setPointerCapture(ev.pointerId); } catch (e) {}
  }

  function resetInteraction() {
    var n = dragNode;
    dragNode = null;
    dragging = false;
    svg.classList.remove("is-panning");
    document.body.style.cursor = "";
    if (!n) return;
    n.pinned = false;
    try { if (n.g) n.g.setAttribute("class", "node-g"); } catch (e) {}
    try { if (n.g && n._pid !== undefined) n.g.releasePointerCapture(n._pid); } catch (e) {}
    n._pid = undefined;
    kick();
  }

  function fitView() {
    if (!nodes.length) return;
    var bb = gRoot.getBBox();
    var rect = svg.getBoundingClientRect();
    var pad = 90;
    var k = Math.min((rect.width - pad * 2) / Math.max(1, bb.width),
                     (rect.height - pad * 2) / Math.max(1, bb.height), 1.15);
    view.k = k;
    view.tx = rect.width / 2 - (bb.x + bb.width / 2) * k;
    view.ty = rect.height / 2 - (bb.y + bb.height / 2) * k;
    applyView();
  }

  /* 初始视图：1:1 真实大小居中（不是"缩放到全塞进屏幕"） */
  function centerAt1() {
    if (!nodes.length) return;
    var rect = svg.getBoundingClientRect();
    var cx = 0, cy = 0;
    nodes.forEach(function (n) { cx += n.x; cy += n.y; });
    cx /= Math.max(1, nodes.length); cy /= Math.max(1, nodes.length);
    view.k = 1;
    view.tx = rect.width / 2 - cx;
    view.ty = rect.height / 2 - cy;
    applyView();
  }

  function bindInteractions() {
    svg.addEventListener("pointerdown", function (e) {
      if (e.target.closest && e.target.closest(".node-g")) return;
      if (e.button !== undefined && e.button !== 0 && e.pointerType === "mouse") return;
      dragging = true; lastX = e.clientX; lastY = e.clientY;
      svg.classList.add("is-panning");
      document.body.style.cursor = "grabbing";
    });

    /* 统一入口 + `buttons` 看门狗（up 事件丢了也能收尾） */
    function onAnyMove(e) {
      if (dragNode) {
        if (e.buttons !== undefined && e.buttons === 0) { resetInteraction(); return; }
        var p = toGraph(e);
        if (Math.abs(p.x - dragNode.x) > 0.5 || Math.abs(p.y - dragNode.y) > 0.5) dragMoved = true;
        dragNode.x = p.x; dragNode.y = p.y;
        kick();
        return;
      }
      if (!dragging) return;
      if (e.buttons !== undefined && e.buttons === 0) { resetInteraction(); return; }
      view.tx += e.clientX - lastX; view.ty += e.clientY - lastY;
      lastX = e.clientX; lastY = e.clientY;
      applyView();
    }
    window.addEventListener("pointermove", onAnyMove, true);
    window.addEventListener("mousemove", function (e) {
      if (dragNode || dragging) onAnyMove(e);
    }, true);

    ["pointerup", "pointercancel", "lostpointercapture", "mouseup",
      "touchend", "touchcancel", "blur"].forEach(function (evt) {
      window.addEventListener(evt, function () { resetInteraction(); }, true);
    });
    document.addEventListener("pointerleave", function (e) {
      if (dragNode && e.buttons === 0) resetInteraction();
    }, true);
    document.addEventListener("visibilitychange", function () {
      if (document.hidden) resetInteraction();
    });
    window.addEventListener("keydown", function (e) {
      if (e.key === "Escape") resetInteraction();
    });

    svg.addEventListener("wheel", function (e) {
      e.preventDefault();
      var rect = svg.getBoundingClientRect();
      var mx = e.clientX - rect.left, my = e.clientY - rect.top;
      var factor = e.deltaY < 0 ? 1.12 : 1 / 1.12;
      var k2 = Math.max(0.25, Math.min(2.6, view.k * factor));
      view.tx = mx - (mx - view.tx) * (k2 / view.k);
      view.ty = my - (my - view.ty) * (k2 / view.k);
      view.k = k2;
      applyView();
    }, { passive: false });

    byId("zoom-in").addEventListener("click", function () { zoomBy(1.18); });
    byId("zoom-out").addEventListener("click", function () { zoomBy(1 / 1.18); });
    byId("zoom-reset").addEventListener("click", fitView);
    function zoomBy(f) {
      var rect = svg.getBoundingClientRect();
      var cx = rect.width / 2, cy = rect.height / 2;
      var k2 = Math.max(0.25, Math.min(2.6, view.k * f));
      view.tx = cx - (cx - view.tx) * (k2 / view.k);
      view.ty = cy - (cy - view.ty) * (k2 / view.k);
      view.k = k2;
      applyView();
    }

    svg.addEventListener("click", function (e) {
      if (e.target === svg || e.target === gRoot) focusNode(null);
    });
    byId("d-close").addEventListener("click", function () { focusNode(null); });

    Array.prototype.slice.call(document.querySelectorAll("#tabs button")).forEach(function (b) {
      b.addEventListener("click", function () { showView(b.getAttribute("data-view")); });
    });

    Array.prototype.slice.call(document.querySelectorAll("#layers button")).forEach(function (b) {
      b.addEventListener("click", function () {
        var k = b.getAttribute("data-layer");
        b.classList.toggle("is-on");
        ui.layers[k === "weak" ? "weak" : k] = b.classList.contains("is-on");
        applyLayers();
        var NAME = { props: "属性", labels: "关系名", weak: "弱证据" };
        var onoff = ui.layers[k] ? "开" : "关";
        var extra = "";
        if (k === "props" && ui.layers.props && !ui.focus) extra = " —— 点一个类，属性会围在它周围";
        if (k === "labels" && ui.layers.labels) extra = " —— 边的名字在边中点上";
        if (k === "weak") extra = " —— 34 条关系全是未验证，开启时虚线边变亮";
        setStatus([h("span", { text: "图层 · " + NAME[k] + "：" + onoff + extra })]);
      });
    });

    var motion = byId("motion-toggle");
    if (motion) motion.addEventListener("click", function () {
      var on = document.body.getAttribute("data-motion") === "on";
      document.body.setAttribute("data-motion", on ? "off" : "on");
      motion.classList.toggle("is-on", !on);
    });

    var search = byId("search");
    search.addEventListener("input", function () {
      var q = search.value.trim().toLowerCase();
      ui.q = q;
      function hit(name, display) {
        return !q || String(display || "").toLowerCase().indexOf(q) >= 0 ||
          String(name).toLowerCase().indexOf(q) >= 0;
      }
      nodeEls.forEach(function (o) { o.g.style.display = hit(o.n.key, o.n.c.display_name) ? "" : "none"; });
      Object.keys(railItems).forEach(function (k) {
        railItems[k].style.display = hit(k, (S.byName[k] || {}).display_name) ? "" : "none";
      });
      Object.keys(cardEls).forEach(function (k) {
        cardEls[k].style.display = hit(k, (S.byName[k] || {}).display_name) ? "" : "none";
      });
    });

    byId("q-run").addEventListener("click", runQuery);
    byId("q-clear").addEventListener("click", function () {
      qText.value = "";
      clear(qOut);
      setStatus([h("span", { text: "就绪" })]);
    });
    qText.addEventListener("keydown", function (e) {
      if ((e.ctrlKey || e.metaKey) && e.key === "Enter") { e.preventDefault(); runQuery(); }
    });

    window.addEventListener("resize", function () {
      var g = byId("v-graph");
      if (g && g.classList.contains("is-on")) applyView();
    });
  }

  /* ── Tooltip ────────────────────────────────────────────────── */
  var tip = byId("tip");
  function showTip(ev, n) {
    var v = verifiability(n.c);
    clear(tip);
    tip.appendChild(h("div", {}, [h("b", { text: n.c.display_name || n.key })]));
    tip.appendChild(h("div", { class: "k", text: iri(n.key) }));
    tip.appendChild(h("div", { style: "margin-top:4px", text:
      v.label + " · " + (n.c.property_count || 0) + " 属性 · " + (n.c.relation_count || 0) + " 关系" }));
    tip.classList.add("is-on");
    moveTip(ev);
  }
  function moveTip(ev) {
    tip.style.left = Math.min(window.innerWidth - 320, ev.clientX + 14) + "px";
    tip.style.top = Math.min(window.innerHeight - 100, ev.clientY + 14) + "px";
  }
  function hideTip() { tip.classList.remove("is-on"); }

  /* ══════════════════════════════════════════════════════════════
     装配
     ══════════════════════════════════════════════════════════════ */

  function renderAll() {
    /* 顶部 */
    var h0 = S.health || {};
    /* 加载态期间 tab 是 disabled 的（壳里没有数据可切）；数据到了才打开。 */
    Array.prototype.slice.call(document.querySelectorAll("#tabs button")).forEach(function (b) {
      b.disabled = false;
    });
    setText("crumb-project", h0.project || "—");
    setText("crumb-sub", h0.ontology_version !== undefined && h0.ontology_version !== null
      ? "第 " + h0.ontology_version + " 版 · 已冻结" : "—");
    var modeBox = byId("badge-mode");
    if (modeBox) {
      modeBox.hidden = false;
      setText("badge-mode-text", h0.mode === "ontology-only" ? "本体模式 · 不接库"
        : (h0.mode || "查询模式"));
    }
    setText("badge-counts", S.counts.classes + " 类 · " + S.counts.relations + " 关系");
    setText("s-classes", S.counts.classes);
    setText("s-rels", S.counts.relations);
    setText("s-props", S.counts.properties);
    setText("rail-count", S.classes.length);
    setText("q-mode-hint", "mode=" + (h0.mode || "?") + " · /api/sparql 默认 JSON-LD");

    /* 图 + 列表 */
    buildGraph();
    syncNodes();
    syncEdges();
    buildLegend();
    buildRail();
    buildCards();
    buildSamples();

    focusNode(null);
    showView("graph");
    centerAt1();
    setStatus([h("span", { text: "就绪" })]);
  }

  function start() {
    bindInteractions();

    byId("token-connect").addEventListener("click", function () {
      var input = byId("token");
      var v = String(input ? input.value : "").trim();
      if (!v) { showAuthNeeded(); return; }
      if (input) input.value = "";        /* ⛔ 不把口令留在 DOM 里 */
      saveToken(v);
    });
    byId("token").addEventListener("keydown", function (e) {
      if (e.key === "Enter") { e.preventDefault(); byId("token-connect").click(); }
    });
    byId("token-forget").addEventListener("click", forgetToken);

    [byId("v-graph"), byId("v-catalog"), byId("v-query")].forEach(function (el) {
      if (el) el.classList.remove("is-on");
    });
    boot();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
  } else {
    start();
  }
})();
