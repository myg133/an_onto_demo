/* ============================================================================
   ontok studio · 效果样机
   ---------------------------------------------------------------
   数据来源（**只用现有 API 的返回形状**，不改后端）：
     GET /api/health
     GET /api/catalog?page=&page_size=   （kind: class | property | relation）
     GET /api/describe?name=             （属性 + 关系）
     POST /api/sparql                    （JSON-LD，含 @pending）

   本文件在 `file://` 下读 snapshot.js 的离线快照；
   若同源起服务（有 CORS / 同域），把 USE_LIVE 改成 true 即走真接口。

   ⛔ 约束：零外部依赖；DOM 一律用 createElement / textContent / createElementNS
   构造（与现有 console 的安全断言一致，不走字符串拼 HTML）。
   ========================================================================== */
(function () {
  "use strict";

  var USE_LIVE = false;                     // 起服务后改 true 即走真接口
  var BASE = (window.ONTOK_SNAPSHOT && window.ONTOK_SNAPSHOT.base_url) || "";
  var TOKEN = "";                           // 生产：来自 sessionStorage（与 console 同款）

  var S = window.ONTOK_SNAPSHOT;
  if (!S) {
    document.body.textContent = "缺少 snapshot.js（先用 fetch-snapshot 生成）";
    return;
  }

  /* ── 极简 DOM 助手（不用字符串拼 HTML）────────────────────────── */
  function h(tag, attrs, kids) {
    var n = document.createElement(tag);
    if (attrs) Object.keys(attrs).forEach(function (k) {
      var v = attrs[k];
      if (v === null || v === undefined) return;
      if (k === "class") n.className = v;
      else if (k === "text") n.textContent = v;
      else if (k.slice(0, 2) === "on") n.addEventListener(k.slice(2), v);
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

  /* ── 建模：把 catalog 的平铺 items 变成图 ─────────────────────── */
  var items = S.catalog.items || [];
  var classes = items.filter(function (i) { return i.kind === "class"; });
  var properties = items.filter(function (i) { return i.kind === "property"; });
  var relations = items.filter(function (i) { return i.kind === "relation"; });
  var counts = S.catalog.counts || {};
  var health = S.health || {};

  var byName = {};
  classes.forEach(function (c) { byName[c.name] = c; });

  var propsOf = {};
  properties.forEach(function (p) {
    (propsOf[p["class"]] || (propsOf[p["class"]] = [])).push(p);
  });

  var relsOf = {};
  relations.forEach(function (r) {
    (relsOf[r.from] || (relsOf[r.from] = [])).push(r);
    (relsOf[r.to] || (relsOf[r.to] = [])).push(r);
  });

  /* ── 关系证据等级：图例按**真实分布**生成，绝不写死 ────────────
     实测发现：本套本体 34 条关系**全部 unverified 且 evidence 为空**
     （文档抽取的语义关系，没有数据实测支撑）⇒ 图必须说实话。 */
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
  var evCount = {};
  relations.forEach(function (r) {
    var k = String(r.evidence_level || "").toLowerCase() || "unknown";
    evCount[k] = (evCount[k] || 0) + 1;
  });

  /* 可查性三态 —— 只用现有字段推导，不新增后端语义 */
  function verifiability(c) {
    var kind = (c.access && c.access.kind) || "db";
    if (kind === "api") return { key: "api", label: "需取数", tone: "warn",
      why: "API 源：平台不执行，查询会给 @pending 取数指引（pushdown: " +
           (((c.access && c.access.pushdown_fields) || []).join(", ") || "无") + "）" };
    if (health.mode === "ontology-only") return { key: "none", label: "不可查", tone: "dim",
      why: "库源，但本部署是本体模式（不接业务库）⇒ 查询会 503 NO_DATA_SOURCE" };
    return { key: "ok", label: "可查", tone: "ok", why: "库源，可走 SPARQL" };
  }

  function acronym(key) {
    var seg = String(key || "").split("_").filter(Boolean);
    if (!seg.length) return "?";
    if (seg[0].length <= 4) return seg[0].toUpperCase();
    if (seg.length > 1) return seg.map(function (x) { return x[0]; }).join("").toUpperCase().slice(0, 3);
    return seg[0].slice(0, 3).toUpperCase();
  }

  /* ── 力导向布局（确定性：按名字排序后固定初始角）───────────── */
  var nodes = classes.map(function (c, i) {
    var a = (i / Math.max(1, classes.length)) * Math.PI * 2;
    var r = 300 + (i % 3) * 34;
    return { key: c.name, c: c, x: Math.cos(a) * r, y: Math.sin(a) * r, vx: 0, vy: 0, deg: 0 };
  });
  var nodeByKey = {};
  nodes.forEach(function (n) { nodeByKey[n.key] = n; });

  var edges = relations.map(function (r) {
    return { r: r, a: nodeByKey[r.from], b: nodeByKey[r.to] };
  }).filter(function (e) { return e.a && e.b; });
  edges.forEach(function (e) { e.a.deg++; e.b.deg++; });

  function simulate() {
    /* 尺度：最近间距 ~105px、画布 ~1290×1340（原来 62px / 681×661，密密麻麻） */
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
  simulate();

  /* ── 标签避让：同屏 30 个类，长中文名会互相压 ──────────────────
     贪心：按 y 排序，纵向依次下挪，直到与已占用矩形不相交。 */
  (function placeLabels() {
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
  })();

  /* ── 图渲染 ─────────────────────────────────────────────────── */
  var svg = document.getElementById("graph");
  var gRoot = s("g", { id: "g-root" });
  var gEdges = s("g");
  var gNodes = s("g");
  var gProps = s("g", { opacity: "0" });
  gRoot.appendChild(gEdges);
  gRoot.appendChild(gProps);
  gRoot.appendChild(gNodes);
  svg.appendChild(gRoot);

  var view = { tx: 0, ty: 0, k: 1 };
  function applyView() {
    gRoot.setAttribute("transform",
      "translate(" + view.tx + "," + view.ty + ") scale(" + view.k + ")");
  }

  var state = { focus: null, hover: null, layers: { props: true, labels: true, weak: false }, q: "" };

  function edgeTone(r) { return evOf(r).tone; }
  function isWeak(r) { return evOf(r).dash; }

  function shortRel(r) {
    var n = String(r.name || "");
    var parts = n.split("__");
    return parts.length > 1 ? parts[1] : n;
  }

  var edgeEls = [];
  edges.forEach(function (e, i) {
    var line = s("path", {
      "class": "edge-line",
      stroke: edgeTone(e.r),
      "stroke-width": String(0.9 + (String(e.r.evidence_level).toLowerCase() === "true" ? 0.5 : 0)),
      "stroke-dasharray": isWeak(e.r) ? "4 4" : null
    });
    line.style.animationDelay = (i * 9) + "ms";
    var lbl = s("text", { "class": "edge-label" });
    lbl.textContent = shortRel(e.r);
    var lbg = s("rect", { "class": "edge-label-bg", rx: "3", height: "13" });
    gEdges.appendChild(line);
    gEdges.appendChild(lbg);
    gEdges.appendChild(lbl);
    edgeEls.push({ e: e, line: line, lbl: lbl, lbg: lbg });
  });

  /* ⭐ 边与标签的几何：**必须真的算 `d`**
     （旧版漏了这一步 ⇒ 只创建了 path 却没路径数据 ⇒ 一条线都不显示） */
  function syncEdges() {
    var placed = [];                       /* 本次渲染已占用的标签矩形（避让） */
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

      /* 可见性：图层开关 & 焦点 & 弱证据 & 避让 */
      var near = !state.focus || o.e.a.key === state.focus || o.e.b.key === state.focus;
      var show = state.layers.labels && near &&
                 (state.layers.weak || !isWeak(o.e.r)) &&
                 len > 74;                       /* 太短的边放不下名字 */
      if (show) {
        var rect = { x0: mx - w / 2 - 1, x1: mx + w / 2 + 1, y0: my - 15, y1: my - 2 };
        for (var i = 0; i < placed.length; i++) {
          var r = placed[i];
          if (!(rect.x1 < r.x0 || rect.x0 > r.x1 || rect.y1 < r.y0 || rect.y0 > r.y1)) { show = false; break; }
        }
        if (show) placed.push(rect);
      }
      o.lbl.setAttribute("opacity", show ? (state.focus ? "0.95" : "0.78") : "0");
      o.lbg.setAttribute("opacity", show ? (state.focus ? "0.9" : "0.75") : "0");
    });
  }

  var view = { tx: 0, ty: 0, k: 1 };
  function applyView() {
    gRoot.setAttribute("transform",
      "translate(" + view.tx + "," + view.ty + ") scale(" + view.k + ")");
  }

  var state = { focus: null, hover: null, layers: { props: true, labels: true, weak: false }, q: "" };

  function edgeTone(r) { return evOf(r).tone; }
  function isWeak(r) { return evOf(r).dash; }


  /* ── defs：立体球用的渐变与阴影 ───────────────────────────── */
  (function injectDefs() {
    var defs = s("defs", {});
    function radialGrad(id, stops) {
      var g = s("radialGradient", { id: id, cx: "34%", cy: "27%", r: "80%" });
      stops.forEach(function (st) {
        g.appendChild(s("stop", { offset: st[0] + "%", "stop-color": st[1],
          "stop-opacity": st[2] === undefined ? "1" : String(st[2]) }));
      });
      defs.appendChild(g);
    }
    /* 球体：亮斑 → 本体色 → 暗边，制造球面感 */
    /* 低对比、高衰减 —— 目标是"能量球"而不是"塑料球" */
    radialGrad("sphere-warn", [[0, "#ffecc0", 0.95], [26, "#e2b45c", 0.9], [62, "#7d5a18", 0.85], [100, "#150f04", 0.92]]);
    radialGrad("sphere-dim", [[0, "#e6eefa", 0.9], [26, "#7d8ca6", 0.85], [62, "#39465c", 0.8], [100, "#0b111b", 0.9]]);
    radialGrad("sphere-ok", [[0, "#e3f6ff", 0.95], [26, "#5fa3cd", 0.9], [62, "#1d526f", 0.85], [100, "#06161f", 0.92]]);
    /* 外发光：中心有色 → 边缘透明 */
    radialGrad("glow-warn", [[0, "#e6b455", 0.55], [55, "#e6b455", 0.16], [100, "#e6b455", 0]]);
    radialGrad("glow-dim", [[0, "#7f8ea8", 0.4], [55, "#7f8ea8", 0.12], [100, "#7f8ea8", 0]]);
    radialGrad("glow-ok", [[0, "#6ec8ff", 0.55], [55, "#6ec8ff", 0.16], [100, "#6ec8ff", 0]]);
    /* 注：曾用 SVG 滤镜做泛光，但它会在 60 个元素上**每帧重算**导致整页闪烁，
       已改为纯径向渐变实现 —— ⛔ 不要再加回来。 */
    svg.appendChild(defs);
  })();

  var nodeEls = [];
  nodes.forEach(function (n) {
    var v = verifiability(n.c);
    var tone = v.key === "api" ? "#e6b455" : v.key === "ok" ? "#6ec8ff" : "#5d6a80";
    var kind = v.key === "api" ? "warn" : v.key === "ok" ? "ok" : "dim";
    var g = s("g", { "class": "node-g", "data-key": n.key });
    n.g = g;                     /* ⭐ 模型持有自己的 <g>：拖拽/复位都要用 */

    /* ① 大范围辉光（呼吸） */
    var halo = s("circle", { "class": "node-halo", cx: n.x, cy: n.y,
      r: 42 + Math.min(16, n.deg * 2.6), fill: "url(#glow-" + kind + ")" });
    /* ② 旋转虚线轨道（科技感的主要来源） */
    var orbit = s("circle", { "class": "node-orbit", cx: n.x, cy: n.y, r: 28.5,
      stroke: tone, "stroke-dasharray": "2 7", "stroke-width": "1.1" });
    /* ③ 球体（低对比，不再"塑料球"） */
    var sphere = s("circle", { "class": "node-sphere", cx: n.x, cy: n.y, r: 21.5,
      fill: "url(#sphere-" + kind + ")" });
    /* ④ 边缘光（rim light） */
    var rim = s("circle", { "class": "node-rim", cx: n.x, cy: n.y, r: 21.5,
      fill: "none", stroke: tone, "stroke-width": "1.1" });
    /* ⑤ 高光（很淡，只提一点体积） */
    var spec = s("ellipse", { "class": "node-spec", cx: n.x - 6.6, cy: n.y - 7.4,
      rx: 6, ry: 4.2, fill: "#fff", opacity: "0.18" });
    /* ⑥ 内圈细线 */
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
    sub.textContent = n.c.property_count + " 属性";

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

    nodeEls.push({ n: n, g: g, halo: halo, orbit: orbit, sphere: sphere,
      rim: rim, spec: spec, ring: ring, core: core, label: label, sub: sub, tone: tone });
  });

  /* ── 节点几何同步（含 z 序：靠下的画在上层，制造前后层次）──────── */
  function syncNodes() {
    /* ⚠️ 这里**绝不重排 DOM**：早期版本按 y 排序 appendChild 做"层次感"，
       结果每次收敛都整组重绘（观感=闪一下）。层次感用 z 序换稳定性，不划算。 */
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
  syncNodes();
  syncEdges();

  /* ══════════════════════════════════════════════════════════════
     物理引擎：斥力 + 弹簧 + 重力 + **碰撞（球不重叠）** + 阻尼
     · 拖动时被拖的节点被"钉住"（pinned），邻居靠弹簧被拉着动
     · 松手后系统自己收敛，**速度低于阈值就停**——静止时一点都不动
     ══════════════════════════════════════════════════════════════ */
  var REST_LEN = 280;          /* 弹簧自然长度（与静态布局一致） */
  var MIN_GAP = 88;            /* 碰撞下限：留出球 + 标签的空间，别贴死 */
  var dragNode = null, dragMoved = false;
  var physics = { running: false, alpha: 1 };

  function applyForces(al) {
    var i, j, a, b, dx, dy, d2, d, f, ux, uy;
    nodes.forEach(function (n) { n.fx = 0; n.fy = 0; });

    /* ① 斥力（库仑） */
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
    /* ② 弹簧（关系边） */
    edges.forEach(function (e) {
      dx = e.b.x - e.a.x; dy = e.b.y - e.a.y;
      d = Math.sqrt(dx * dx + dy * dy) || 1;
      f = (d - REST_LEN) * 0.016 * al;
      ux = dx / d; uy = dy / d;
      e.a.fx += ux * f; e.a.fy += uy * f;
      e.b.fx -= ux * f; e.b.fy -= uy * f;
    });
    /* ③ 极轻向心（防整体飘走，比斥力小两个数量级） */
    nodes.forEach(function (n) { n.fx -= n.x * 0.0016 * al; n.fy -= n.y * 0.0016 * al; });
  }

  function integrate() {
    nodes.forEach(function (n) {
      if (n.pinned) { n.vx = 0; n.vy = 0; return; }
      n.vx = (n.vx + n.fx) * 0.72;          /* 黏滞阻尼 ⇒ 有重量感 */
      n.vy = (n.vy + n.fy) * 0.72;
      n.x += n.vx; n.y += n.vy;
    });
    /* ④ 碰撞：硬约束，球体不重叠 */
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

  /* 退火（cooling）：alpha 每帧衰减 ⇒ **力学上保证会停**，
     不会像"每帧加一点力"那样永远在抖。 */
  function tick() {
    physics.alpha = dragNode ? Math.max(physics.alpha * 0.94, 0.5)
                             : physics.alpha * 0.94;
    applyForces(physics.alpha);
    integrate();
    syncNodes(); syncEdges(); renderProps();
    if (!dragNode && physics.alpha < 0.03) {      /* 收敛 ⇒ 停：静止时零运动 */
      physics.running = false;
      return;
    }
    window.requestAnimationFrame(tick);
  }
  function kick() {
    if (!physics.running) {
      physics.running = true;
      /* ⚠️ 千万**不要**重置成 1：那会让整张图重新收敛（实测最大位移 179px，
         观感就是"松手后闪一下"）。只给一个够用的下限即可。 */
      physics.alpha = Math.max(physics.alpha, 0.5);
      window.requestAnimationFrame(tick);
    }
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
    /* 指针捕获：按在节点上之后 move/up 都回到它（浏览器不支持也不影响兜底） */
    try { if (n.g) n.g.setPointerCapture(ev.pointerId); } catch (e) {}
  }

  function resetInteraction() {
    var n = dragNode;
    dragNode = null;             /* ⭐ 先清状态：后面任何一步抛异常都不会再"粘住" */
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

  /* 属性小点：仅当聚焦某类时显示它的属性（最多 8 个 + N） */
  function renderProps() {
    while (gProps.firstChild) gProps.removeChild(gProps.firstChild);
    if (!state.focus) { gProps.setAttribute("opacity", "0"); return; }
    var n = nodeByKey[state.focus];
    var list = (propsOf[state.focus] || []).slice(0, 8);
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
      t.textContent = String(p.name || "").split("__").pop();
      gProps.appendChild(dot); gProps.appendChild(t);
    });
    gProps.setAttribute("opacity", "1");
  }

  /* ── 聚焦 / 淡化 / 高亮邻居 ─────────────────────────────────── */
  function focusNode(key) {
    state.focus = key;
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
      o.g.classList.toggle("is-focus", state.focus === k);
      o.g.classList.toggle("is-near", !!state.focus && state.focus !== k && near[k]);
      o.g.classList.toggle("is-faded", !!state.focus && !near[k]);
    });
    edgeEls.forEach(function (o) {
      var on = !state.focus || o.e.a.key === state.focus || o.e.b.key === state.focus;
      o.line.setAttribute("opacity", on ? "0.8" : "0.06");
    });
    syncEdges();                       /* 标签可见性只在这里决定 */
    renderProps();
    if (key) openDrawer(key); else closeDrawer();
    highlightRail(key);
  }

  /* ── 抽屉：类详情（属性 / 关系 / 可查性 / 证据）───────────── */
  var drawer = document.getElementById("drawer");
  var dTitle = document.getElementById("d-title");
  var dKey = document.getElementById("d-key");
  var dBody = document.getElementById("d-body");

  function openDrawer(key) {
    var c = byName[key];
    if (!c) return;
    var v = verifiability(c);
    dTitle.textContent = c.display_name || key;
    dKey.textContent = ":" + key;
    while (dBody.firstChild) dBody.removeChild(dBody.firstChild);

    /* 概况 */
    dBody.appendChild(h("div", { class: "sect" }, [
      h("div", { class: "sect-h", text: "概况" }),
      h("dl", { class: "kv" }, [
        h("dt", { text: "可查性" }),
        h("dd", {}, [h("span", { class: "pill " + (v.key === "api" ? "api" : "db"), text: v.label })]),
        h("dt", { text: "说明" }), h("dd", { text: v.why }),
        h("dt", { text: "规模" }), h("dd", {
          text: (c.property_count || 0) + " 属性 · " + (c.relation_count || 0) + " 关系" }),
        c.description ? h("dt", { text: "定义" }) : null,
        c.description ? h("dd", { text: c.description }) : null
      ])
    ]));

    /* 属性 */
    var props = (propsOf[key] || []);
    if (props.length) {
      dBody.appendChild(h("div", { class: "sect" }, [
        h("div", { class: "sect-h", text: "属性 · " + props.length }),
        h("div", { class: "prop-list" }, props.map(function (p) {
          return h("div", { class: "prop" }, [
            h("span", { class: "pn", text: p.display_name || String(p.name || "").split("__").pop() }),
            h("span", { class: "pk", text: String(p.name || "").split("__").pop() }),
            h("span", { class: "pt", text: p.column ? "列" : "—" })
          ]);
        }))
      ]));
    }

    /* 关系（带证据等级） */
    var rels = (relsOf[key] || []);
    if (rels.length) {
      dBody.appendChild(h("div", { class: "sect" }, [
        h("div", { class: "sect-h", text: "关系 · " + rels.length }),
        h("div", { class: "rel-list" }, rels.map(function (r) {
          var ev = evOf(r);
          var cls = ev.cls;
          var txt = ev.label;
          var other = r.from === key ? r.to : r.from;
          var dir = r.from === key ? "→" : "←";
          return h("div", { class: "rel" }, [
            h("b", { text: shortRel(r) }),
            h("span", { class: "arrow", text: dir }),
            h("span", { text: other }),
            h("span", { class: "ev " + cls, text: txt })
          ]);
        }))
      ]));
    }

    drawer.classList.add("is-open");
  }
  function closeDrawer() { drawer.classList.remove("is-open"); }

  /* ── 图例：按真实数据生成（外加"全部未验证"的诚实提示）───────── */
  (function buildLegend() {
    var box = document.getElementById("legend");
    if (!box) return;
    var apiN = classes.filter(function (c) { return (c.access || {}).kind === "api"; }).length;
    var rows = [];
    rows.push(h("div", { class: "legend-row" }, [
      h("span", { class: "legend-swatch", style: "color:#e6b455;background:#e6b455" }),
      h("span", { text: "API 源 " + apiN + " 类 · 平台不执行，按 get 自取" })
    ]));
    rows.push(h("div", { class: "legend-row" }, [
      h("span", { class: "legend-swatch", style: "color:#5d6a80;background:#5d6a80" }),
      h("span", { text: "库源 " + (classes.length - apiN) + " 类 · 本部署无库，查询 503" })
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
    var allUnverified = Object.keys(evCount).length === 1 && evCount.unverified === relations.length;
    if (allUnverified) {
      rows.push(h("div", {
        class: "legend-row",
        style: "color:#e6b455;align-items:flex-start;line-height:1.45;margin-top:2px"
      }, [h("span", {
        text: "⚠️ 全部 " + relations.length + " 条关系都是"
          + "文档推断、无实测证据（evidence 为空）—— 当作语义提示，不要当已验证的事实。"
      })]));
    }
    rows.forEach(function (r) { box.appendChild(r); });
  })();

  /* ── 左侧栏列表 ─────────────────────────────────────────────── */
  var railList = document.getElementById("rail-list");
  var railItems = {};

  classes.slice().sort(function (a, b) {
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
    row.addEventListener("click", function () { focusNode(c.name); });
    railList.appendChild(row);
    railItems[c.name] = row;
  });

  function highlightRail(key) {
    Object.keys(railItems).forEach(function (k) {
      railItems[k].classList.toggle("is-active", k === key);
      railItems[k].classList.remove("is-dim");
    });
    if (key) {
      edges.forEach(function (e) {
        if (e.a.key === key && railItems[e.b.key]) railItems[e.b.key].classList.remove("is-dim");
        if (e.b.key === key && railItems[e.a.key]) railItems[e.a.key].classList.remove("is-dim");
      });
      Object.keys(railItems).forEach(function (k) {
        var near = k === key ||
          edges.some(function (e) {
            return (e.a.key === key && e.b.key === k) || (e.b.key === key && e.a.key === k);
          });
        if (!near) railItems[k].classList.add("is-dim");
      });
    }
  }

  /* ── 目录视图 ───────────────────────────────────────────────── */
  var cards = document.getElementById("cards");
  var cardEls = {};
  (function buildCards() {
    classes.forEach(function (c) {
      var v = verifiability(c);
      var card = h("div", { class: "card", "data-key": c.name }, [
        h("div", { class: "card-top" }, [
          h("div", { class: "card-name", text: c.display_name || c.name }),
          h("span", { class: "pill " + (v.key === "api" ? "api" : "db"), text: v.label })
        ]),
        h("div", { class: "card-key", text: c.name }),
        h("div", { class: "card-meta" }, [
          h("span", {}, [h("b", { text: String(c.property_count || 0) }), " 属性"]),
          h("span", {}, [h("b", { text: String(c.relation_count || 0) }), " 关系"])
        ])
      ]);
      card.addEventListener("click", function () { focusNode(c.name); });
      cards.appendChild(card);
      cardEls[c.name] = card;
    });
    document.getElementById("catalog-sub").textContent =
      classes.length + " 个类 · 其中 " +
      classes.filter(function (c) { return (c.access || {}).kind === "api"; }).length +
      " 个来自 API 源（平台不执行，给取数指引）";
  })();

  /* ── 查询视图（渲染真实响应；离线用快照样本）────────────────── */
  var samples = S.samples || {};
  var qText = document.getElementById("q-text");
  var qOut = document.getElementById("q-out");
  var qStatus = document.getElementById("q-status");

  var PRESETS = [
    { label: "api 类 · 应给 @pending", q: "SELECT ?c WHERE { ?c a :cvm_instance } LIMIT 3", key: "cvm_jsonld" },
    { label: "api 类 · rows 模式应 400", q: "SELECT ?c WHERE { ?c a :cvm_instance } LIMIT 3", key: "cvm_rows_error", rows: true },
    { label: "库类 · 应 503（本部署无库）", q: "SELECT ?c WHERE { ?c a :alarm_notice } LIMIT 3", key: "db_class_503" }
  ];
  qText.value = PRESETS[0].q;

  var sampleBar = document.getElementById("q-samples");
  PRESETS.forEach(function (p) {
    var b = h("button", { class: "q-sample", text: p.label });
    b.addEventListener("click", function () {
      qText.value = p.q;
      runSample(p);
    });
    sampleBar.appendChild(b);
  });

  function chip(status) {
    var cls = status === 200 ? "chip-200" : status === 400 ? "chip-400" : "chip-503";
    return h("span", { class: "chip-code " + cls, text: "HTTP " + status });
  }

  function renderJson(v, keyName) {
    var frag = document.createDocumentFragment();
    function walk(x, depth, pad, name) {
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
          walk(x[k], depth + 1, pad + 1, k);
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
    walk(v, 0, 0, keyName);
    return frag;
  }

  function runSample(p) {
    var smp = samples[p.key];
    if (!smp) { renderOut(null, "样本缺失"); return; }
    renderOut(smp, null);
  }

  function renderOut(smp, note) {
    while (qOut.firstChild) qOut.removeChild(qOut.firstChild);
    if (!smp) {
      qOut.appendChild(h("div", { text: note || "无响应" }));
      return;
    }
    var body = smp.body;
    /* @pending 高亮说明 —— 这是本产品最容易被误读的地方 */
    if (body && body["@pending"] && body["@pending"].length) {
      var g0 = body["@pending"][0] || {};
      var get = g0.get || {};
      qOut.appendChild(h("div", { class: "pending-box" }, [
        h("div", {}, [h("b", { text: "这次查询没给数据，给的是取数指引（@pending）" })]),
        h("div", { style: "margin-top:6px", text:
          "平台不执行 API 源。get.class = " + (get["class"] || "?") +
          "，可下推 " + ((get.pushdown || []).join(" / ") || "无") +
          "，凭 ref 追溯映射版本。" }),
        h("div", { style: "margin-top:6px" }, [
          h("span", { text: "⛔ filter 为空时不要全量拉 —— 先用 " }),
          h("code", { text: "describe" }),
          h("span", { text: " 找到标识属性，再带 ids 取。" })
        ]),
        h("div", { style: "margin-top:6px;opacity:.8" }, [
          h("span", { text: "meta.complete = " }),
          h("code", { text: String((body.meta || {}).complete) }),
          h("span", { text: " ⇒ 这份结果**不完整**，不能直接当答案。" })
        ])
      ]));
    }
    qOut.appendChild(h("div", { style: "font-family:var(--mono);font-size:11.5px;white-space:pre" },
      [renderJson(body)]));
    while (qStatus.firstChild) qStatus.removeChild(qStatus.firstChild);
    qStatus.appendChild(chip(smp.status));
    qStatus.appendChild(h("span", { text: smp.status === 200 ? "application/ld+json" : (body && body.code) || "" }));
    qStatus.appendChild(h("span", { text: "离线快照（真响应）" }));
  }

  document.getElementById("q-run").addEventListener("click", function () {
    var p = PRESETS.filter(function (x) { return x.q === qText.value; })[0];
    runSample(p || PRESETS[0]);
  });
  document.getElementById("q-clear").addEventListener("click", function () {
    qText.value = "";
    while (qOut.firstChild) qOut.removeChild(qOut.firstChild);
  });
  runSample(PRESETS[0]);

  document.getElementById("q-mode-hint").textContent =
    "mode=" + (health.mode || "?") + " · 样本为真实响应";

  /* ── 视图切换 ───────────────────────────────────────────────── */
  var views = { graph: "v-graph", catalog: "v-catalog", query: "v-query" };
  var tabBtns = Array.prototype.slice.call(document.querySelectorAll("#tabs button"));
  function showView(name) {
    Object.keys(views).forEach(function (k) {
      document.getElementById(views[k]).classList.toggle("is-on", k === name);
    });
    tabBtns.forEach(function (b) { b.classList.toggle("is-on", b.getAttribute("data-view") === name); });
    if (name !== "graph") closeDrawer();
    if (name === "graph") fitView();
  }
  tabBtns.forEach(function (b) {
    b.addEventListener("click", function () { showView(b.getAttribute("data-view")); });
  });

  /* ── 图层开关 ───────────────────────────────────────────────── */
  Array.prototype.slice.call(document.querySelectorAll("#layers button")).forEach(function (b) {
    b.addEventListener("click", function () {
      var k = b.getAttribute("data-layer");
      b.classList.toggle("is-on");
      state.layers[k === "weak" ? "weak" : k] = b.classList.contains("is-on");
      if (k === "props") {
        /* 属性图层关掉时清空属性小点 */
        if (!state.layers.props) { while (gProps.firstChild) gProps.removeChild(gProps.firstChild); gProps.setAttribute("opacity", "0"); }
        else renderProps();
      } else {
        focusNode(state.focus);
      }
    });
  });

  /* ── 动效开关（默认关：持续动画会让整页重绘）─────────────────── */
  (function motionToggle() {
    var btn = document.getElementById("motion-toggle");
    if (!btn) return;
    btn.addEventListener("click", function () {
      var on = document.body.getAttribute("data-motion") === "on";
      document.body.setAttribute("data-motion", on ? "off" : "on");
      btn.classList.toggle("is-on", !on);
    });
  })();

  /* ── 平移 / 缩放 ─────────────────────────────────────────────── */
  function fitView() {
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

  /* ── 初始视图：**1:1 真实大小居中**（不是"缩放到全塞进屏幕"）
     —— 后者会把放大的布局又缩回去，屏幕上永远一样挤 ─────────────── */
  function centerAt1() {
    var rect = svg.getBoundingClientRect();
    var cx = 0, cy = 0;
    nodes.forEach(function (n) { cx += n.x; cy += n.y; });
    cx /= Math.max(1, nodes.length); cy /= Math.max(1, nodes.length);
    view.k = 1;
    view.tx = rect.width / 2 - cx;
    view.ty = rect.height / 2 - cy;
    applyView();
  }

  var dragging = false, lastX = 0, lastY = 0;

  svg.addEventListener("pointerdown", function (e) {
    if (e.target.closest && e.target.closest(".node-g")) return;
    if (e.button !== undefined && e.button !== 0 && e.pointerType === "mouse") return;
    dragging = true; lastX = e.clientX; lastY = e.clientY;
    svg.classList.add("is-panning");
    document.body.style.cursor = "grabbing";
  });

  /* 统一入口：任何移动都走这里，并且**用 buttons 做看门狗**
     —— 只要发现"键已经不按了"，立刻结束拖拽（哪怕 up 事件丢了） */
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

  /* 结束路径**四条**（任何一条到就收）：up / cancel / 丢捕获 / 窗口失焦
     + 触摸结束 + 离开文档 + 切标签页 + Esc */
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

  document.getElementById("zoom-in").addEventListener("click", function () { zoomBy(1.18); });
  document.getElementById("zoom-out").addEventListener("click", function () { zoomBy(1 / 1.18); });
  document.getElementById("zoom-reset").addEventListener("click", fitView);
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
  document.getElementById("d-close").addEventListener("click", function () { focusNode(null); });

  /* ── 搜索 ───────────────────────────────────────────────────── */
  var search = document.getElementById("search");
  search.addEventListener("input", function () {
    var q = search.value.trim().toLowerCase();
    state.q = q;
    nodeEls.forEach(function (o) {
      var hit = !q ||
        String(o.n.c.display_name || "").toLowerCase().indexOf(q) >= 0 ||
        o.n.key.toLowerCase().indexOf(q) >= 0;
      o.g.style.display = hit ? "" : "none";
    });
    Object.keys(railItems).forEach(function (k) {
      var c = byName[k];
      var hit = !q || String(c.display_name || "").toLowerCase().indexOf(q) >= 0 ||
        k.toLowerCase().indexOf(q) >= 0;
      railItems[k].style.display = hit ? "" : "none";
    });
    Object.keys(cardEls).forEach(function (k) {
      var c = byName[k];
      var hit = !q || String(c.display_name || "").toLowerCase().indexOf(q) >= 0 ||
        k.toLowerCase().indexOf(q) >= 0;
      cardEls[k].style.display = hit ? "" : "none";
    });
  });

  /* ── Tooltip ────────────────────────────────────────────────── */
  var tip = document.getElementById("tip");
  function showTip(ev, n) {
    var v = verifiability(n.c);
    while (tip.firstChild) tip.removeChild(tip.firstChild);
    tip.appendChild(h("div", {}, [h("b", { text: n.c.display_name || n.key })]));
    tip.appendChild(h("div", { class: "k", text: ":" + n.key }));
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

  /* ── 顶部信息 ───────────────────────────────────────────────── */
  document.getElementById("crumb-project").textContent = S.health.project || "—";
  document.getElementById("crumb-sub").textContent =
    "第 " + (S.health.ontology_version || "?") + " 版 · 已冻结";
  document.getElementById("badge-mode-text").textContent =
    S.health.mode === "ontology-only" ? "本体模式 · 不接库" : (S.health.mode || "—");
  document.getElementById("badge-counts").textContent =
    counts.classes + " 类 · " + counts.relations + " 关系";
  document.getElementById("s-classes").textContent = counts.classes;
  document.getElementById("s-rels").textContent = counts.relations;
  document.getElementById("s-props").textContent = counts.properties;
  document.getElementById("rail-count").textContent = counts.classes;

  showView("graph");
  focusNode(null);
  centerAt1();                     /* 初始按 1:1 显示，图比屏幕大 ⇒ 可拖动探索 */          /* 应用初始可见性：关系名默认隐藏 */
  window.addEventListener("resize", function () {
    /* 只重算居中，**不重新 fit**：否则拖完窗口图会突然缩一圈 */
    if (document.getElementById("v-graph").classList.contains("is-on")) applyView();
  });
})();
