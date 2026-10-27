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
    sparql: "/api/sparql",
    /* ⭐ REQ-159 P1c-2 · 管理端点（ADR-013：8 条写在 query serve 上，Studio 调） */
    projects: "/api/projects"
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
      /* ⚠️ 先解析再判 ok：错误体里的 `code` 是**降级依据**
         （`key.not_admin` ⇒ 提示「需要 admin key」；404 ⇒「服务端未就绪」）。
         不解析就只能看 HTTP 状态码，而 403 有两种完全不同的意思。 */
      var parsed = null;
      try { parsed = JSON.parse(text); } catch (e) {}
      if (!res.ok) {
        var msg = (parsed && (parsed.message || parsed.code))
          ? (parsed.code + "：" + parsed.message + (parsed.hint ? "（" + parsed.hint + "）" : ""))
          : text;
        var err = new Error("HTTP " + res.status + " — " + String(msg).slice(0, 400));
        err.status = res.status;
        err.code = (parsed && parsed.code) || "";
        throw err;
      }
      if (!parsed) throw new Error("响应不是 JSON（HTTP " + res.status + "）");
      return parsed;
    });
  }

  /* 写请求（POST / PUT）：body 既可以是 JSON 字符串，也可以是**原始字节**
     （上传构建包就是 `File.arrayBuffer()` 出来的那一份） */
  function apiSend(method, path, body, contentType) {
    var headers = {
      "Authorization": "Bearer " + S.token,
      "Accept": "application/json"
    };
    if (contentType) headers["Content-Type"] = contentType;
    return fetch(path, { method: method, headers: headers, body: body }).then(readJson);
  }

  /* 目录：一页一页翻到 has_more = false（⛔ 不假设一次就够）
     ⭐ REQ-159 P1c-2：`project` 非空时带 `?project=<名>`（admin 切项目看图；服务端
     只对 `kind=admin` 生效，query key 给了这个参数会是 403 `key.project_pinned`）。 */
  function fetchCatalog(onPage, project) {
    var acc = [];
    var counts = null;
    var suffix = project ? ("&project=" + encodeURIComponent(project)) : "";

    function one(page) {
      return apiGet(API.catalog + "?page=" + page + "&page_size=" + PAGE_SIZE + suffix).then(function (p) {
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
    if (!stored) {
      /* 没口令：图看不到（服务端 401），但顶栏那几个入口**存在且禁用**，
         并明确说是「需要 admin key」——⛔ 不是悄悄消失（用户会以为功能没做）。 */
      OPS.admin = false;
      OPS.reason = "unauthorized";
      showAuthNeeded();
      applyOpsGate();
      renderProjectPick();
      return;
    }
    S.token = stored;
    authBadge("", "连接中…");
    showBoot("加载中…", "正在读取项目列表与本体目录…");
    applyOpsGate();

    // health 免口令；失败不致命（页面仍可用目录与查询）。
    var health = apiGet(API.health).catch(function () { return null; });

    Promise.all([fetchProjects(), health]).then(function (out) {
      var h0 = out[1] || {};
      /* ⭐ admin：**先定看哪个项目**，再带 `?project=` 拉目录。
         多项目模式下 admin 不带这个参数会得到 403 `key.admin_has_no_project` ——
         那不是错误，是「你还没说看哪一个」（P1c-1 的契约）。 */
      var project = null;
      if (OPS.admin) {
        project = pickInitialProject(h0.project);
        if (!project) {
          S.health = h0;
          showBoot("还没有项目", "这台服务上还没有任何项目。用顶栏 ＋ 建一个，再上传构建包。");
          renderProjectPick();
          applyOpsGate();
          return null;
        }
        OPS.current = project;
      }
      return fetchCatalog(function (n, page) {
        setText("boot-sub", "已读取 " + n + " 条目录（第 " + page + " 页）…");
      }, project ? project.name : null).then(function (cat) {
        S.items = cat.items;
        S.counts = cat.counts;
        S.health = h0;
        S.loaded = true;
        authBadge("is-live", OPS.admin ? "已连接 · admin" : "已连接");
        var forget = byId("token-forget");
        if (forget) forget.hidden = false;
        buildModel();
        renderAll();
        applyOpsGate();
        if (!OPS.admin) {
          /* ⭐ AC-d8 / AC-d9：不是 admin 也**照常看图**，只把写操作禁用并说清原因；
             ⛔ 不登出（口令是真的）、⛔ 不白屏（图已经画出来了）。 */
          showBanner(OPS.reason === "not_ready" ? "err" : "info",
            OPS.reason === "not_ready"
              ? "服务端未就绪：项目接口不在（404）。图可以看，建项目 / 上传 / 解析 / 激活已禁用。"
              : "需要 admin key：图可以看，但建项目 / 上传 / 解析 / 激活 / 查看 key 已禁用（当前口令不是 admin）。");
        }
      });
    }).catch(function (e) {
      if (e && e.unauthorized) { showAuthInvalid(); applyOpsGate(); return; }
      showLoadError(e && e.message ? e.message : String(e));
      applyOpsGate();
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
         ⭐ REQ-156：有关系语义时优先显示中文标签（catalog 的 `semantic_label`）；
         认不出的动词没有标签，就退回**真名**（英文 key / `from__to`）。
         ⛔ 两条都空时也不许留白 —— 最后用 `semantic`，再不行才给一句占位。 */
      var labelText = String(e.r.semantic_label || "");
      var fullName = labelText || String(e.r.name || "") || String(e.r.semantic || "") || "关系";
      lbl.textContent = fullName.length > 18 ? fullName.slice(0, 17) + "…" : fullName;
      lbl.setAttribute("title", fullName);
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
    apiGet(API.describe + "?name=" + encodeURIComponent(iri(key)) + projectArg("&")).then(function (d) {
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
                   （目标类名），看起来像"关系没有名字"。
             ⭐ REQ-156：有中文标签 ⇒ `标签` + 小字英文 key；没有标签 ⇒ **只显示英文 key**
             （⛔ 不假装是中文）。两条路都不留白。 */
          var label = String(r.semantic_label || "");
          var keyText = String(r.semantic || "") || String(r.name || "");
          var kids = [
            h("span", { class: "arrow", text: dir }),
            h("b", { text: other })
          ];
          if (label) {
            kids.push(h("span", { class: "sem", text: label }));
            if (keyText) kids.push(h("span", { class: "pk", text: keyText }));
          } else {
            kids.push(h("span", { class: "pk", text: keyText || "关系名未知" }));
          }
          kids.push(h("span", { class: "ev " + ev.cls, text: ev.label }));
          return h("div", { class: "rel" }, kids);
        }))
      ]));
    }

    drawer.classList.add("is-open");
  }

  /* 图例：按真实数据生成（外加"全部未验证"的诚实提示） */
  function buildLegend() {
    /* ⭐ REQ-156：本体**有**语义动词了（34 条 / 15 种），边标签优先用 catalog 的
       `semantic_label`；未知动词没有标签就退回英文 key —— ⛔ 别在 UI 里"编"一个好看的动词。 */
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
    fetch(API.sparql + projectQs(), {
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
     ⭐ REQ-159 P1c-2 · 项目 / 构建包（admin 写面，ADR-013）
     ──────────────────────────────────────────────────────────────
     顶栏项目下拉（GET /api/projects；切换 ⇒ 带 ?project= 重拉 catalog 并**局部重绘**）
     ＋ 建项目（POST /api/projects → 一次性 key → 直接进上传向导）
     上传（POST …/bundles，Content-Type: application/octet-stream，body = 原始字节）
     配 DSN（只有 requires_db 才出现；PUT …/database + POST …/database/test）
     解析（POST …/import；already_imported:true 当**成功**）
     激活（POST …/activate；二次确认，文案写明「会影响对外服务」）
     查看 key（GET …/key；固定一行「本次查看会记录一条审计」）

     降级：404 ⇒「服务端未就绪」；403 `key.not_admin` ⇒「需要 admin key」。
     ⛔ 不登出、⛔ 不白屏 —— 图是读路径，写操作才是 admin 的。
     口令仍只进 sessionStorage（⛔ URL、⛔ 长期存储、⛔ 回填 DOM）。
     ══════════════════════════════════════════════════════════════ */

  var OPS = {
    admin: false,        // 真的拿到过项目列表 ⇒「是 admin」
    checked: false,      // 判过了（避免 UI 在「还没问」时就说没权限）
    reason: "",          // not_admin | not_ready | unauthorized | forbidden | error:NNN
    projects: [],
    current: null        // 当前看的项目（含 active_version / 计数）
  };

  /* 构建包向导的状态（一个弹窗，五步） */
  var W = {
    open: false, step: 2, startStep: 2,
    name: "", label: "", keyLabel: "", key: "",
    created: false, err: "",
    uploaded: null, uploading: false, uploadError: "",
    bundleId: "", requiresDb: false,
    dsn: "", dbSaving: false, dbTested: false, dbOk: false, dbResult: null, dbError: "",
    imported: null, importing: false, importError: "", importNote: "",
    activating: false, confirmActivate: false, activated: null, activateError: ""
  };

  var KEYM = { open: false, loading: false, data: null, error: "" };

  var WIZ_STEPS = [
    { n: 1, t: "建项目" },
    { n: 2, t: "上传包" },
    { n: 3, t: "配置 DB" },
    { n: 4, t: "解析" },
    { n: 5, t: "激活" }
  ];
  var WIZ_TITLES = ["", "新建项目", "上传构建包", "配置源数据库", "解析构建包", "冻结并激活"];
  var WIZ_SUBS = ["",
    "一个项目 = 一套本体；部署只有一份。key 只显示这一次。",
    "包是构建端产出的 .ontokbundle。上传只做校验，不解析。",
    "包里含 db（requires_dsn = true）⇒ 必须先配通（没测通不许解析）。",
    "单事务：中途失败一个字节都不落。解析 ≠ 生效。",
    "解析 ≠ 生效：激活才对外可见。"];

  function num(v) { return (v === null || v === undefined) ? 0 : v; }
  function projBase(name) { return API.projects + "/" + encodeURIComponent(name); }

  function findProject(name) {
    for (var i = 0; i < OPS.projects.length; i++) {
      if (OPS.projects[i].name === name) return OPS.projects[i];
    }
    return null;
  }

  function projectMeta(p) {
    if (!p) return "未连接";
    if (p.active_version === null || p.active_version === undefined) {
      return "未激活 · " + num(p.entity_types) + " 类（先上传包并激活）";
    }
    return "第 " + p.active_version + " 版 · " + (p.frozen ? "已冻结" : "未冻结")
      + " · " + num(p.entity_types) + " 类 / " + num(p.link_types) + " 关系";
  }

  function projectMetaShort(p) {
    if (p.active_version === null || p.active_version === undefined) return "未激活";
    return "v" + p.active_version + " · " + num(p.entity_types) + " 类";
  }

  function errText(e) {
    if (!e) return "未知错误";
    if (e.code) return e.code + "：" + String(e.message || "").slice(0, 300);
    return String(e.message || e).slice(0, 300);
  }

  /* ── 顶栏提示（一行文本；admin 正常时是空的） ─────────────────── */
  function setOpsHint(text, isErr) {
    var n = byId("ops-hint");
    if (!n) return;
    n.textContent = text || "";
    n.className = "proj-hint" + (isErr ? " is-err" : "");
  }

  /* ── 图上的提示条（结果 / 降级原因） ──────────────────────────── */
  function showBanner(kind, text) {
    var b = byId("ops-banner");
    if (!b) return;
    clear(b);
    b.className = "ops-banner is-on" + (kind === "err" ? " is-err" : (kind === "ok" ? " is-ok" : ""));
    b.appendChild(h("b", { text: kind === "err" ? "✕" : (kind === "ok" ? "✓" : "·") }));
    b.appendChild(h("span", { text: text }));
    var close = h("button", { class: "btn btn-sm btn-ghost", text: "关闭" });
    close.style.marginLeft = "auto";
    close.addEventListener("click", hideBanner);
    b.appendChild(close);
  }
  function hideBanner() {
    var b = byId("ops-banner");
    if (!b) return;
    clear(b);
    b.className = "ops-banner";
  }

  /* ── admin 判定：/api/projects 是**唯一**的探针 ─────────────────── */
  function classifyOps(e) {
    OPS.checked = true;
    OPS.admin = false;
    OPS.projects = [];
    if (!e) { OPS.reason = "error"; return; }
    if (e.unauthorized) { OPS.reason = "unauthorized"; return; }
    if (e.status === 403) {
      /* 契约码 `key.not_admin`：这张 key 是真的，只是不是写面的钥匙。
         ⛔ 不登出（401 才清口令）—— 它还能读自己绑定的项目。 */
      OPS.reason = (e.code === "key.not_admin") ? "not_admin" : "forbidden";
      return;
    }
    if (e.status === 404) { OPS.reason = "not_ready"; return; }
    OPS.reason = "error:" + (e.status || "?");
  }

  function fetchProjects() {
    return apiGet(API.projects).then(function (v) {
      OPS.checked = true;
      OPS.admin = true;
      OPS.reason = "";
      OPS.projects = (v && Array.isArray(v.projects)) ? v.projects : [];
      if (OPS.current) OPS.current = findProject(OPS.current.name) || OPS.current;
      return OPS.projects;
    }, function (e) {
      classifyOps(e);
      return null;
    });
  }

  function pickInitialProject(healthProject) {
    var list = OPS.projects;
    if (!list.length) return null;
    if (healthProject) {
      for (var i = 0; i < list.length; i++) {
        if (list[i].name === healthProject) return list[i];
      }
    }
    /* 多项目模式下 health 的 project 是 null ⇒ 优先挑一个已有生效版本的 */
    for (var j = 0; j < list.length; j++) {
      if (list[j].active_version !== null && list[j].active_version !== undefined) return list[j];
    }
    return list[0];
  }

  function opsHintText() {
    if (OPS.admin) return "";
    if (OPS.reason === "not_ready") return "服务端未就绪（404）：项目接口不在，写操作已禁用";
    if (OPS.reason === "not_admin") return "需要 admin key：当前口令只能看图";
    if (OPS.reason === "unauthorized") return "需要 admin key：输入后可用";
    if (OPS.reason === "forbidden") return "需要 admin key：当前口令无权列项目";
    return "项目列表不可用，写操作已禁用";
  }

  function setBtnDisabled(id, off) {
    var b = byId(id);
    if (b) b.disabled = !!off;
  }

  /* AC-d8：没 admin 口令 ⇒ 这些按钮**禁用**并提示；有 ⇒ 可用 */
  function applyOpsGate() {
    var on = !!OPS.admin;
    setBtnDisabled("proj-add", !on);
    setBtnDisabled("bundle-open", !on);
    setBtnDisabled("key-view", !on);
    var pick = byId("proj-pick");
    if (pick) pick.disabled = !on || OPS.projects.length === 0;
    setOpsHint(opsHintText(), !on);
  }

  function renderProjectPick() {
    var cur = OPS.current;
    var h0 = S.health || {};
    if (cur) {
      setText("crumb-project", cur.name);
      setText("crumb-sub", projectMeta(cur));
    } else {
      /* 没有项目列表时（非 admin）退回 health 说的那一个 —— 与改造前显示同一句话 */
      setText("crumb-project", h0.project || "—");
      setText("crumb-sub", (h0.ontology_version !== undefined && h0.ontology_version !== null)
        ? ("第 " + h0.ontology_version + " 版 · 已冻结") : "—");
    }
    renderProjectMenu();
  }

  function renderProjectMenu() {
    var box = byId("proj-menu");
    if (!box) return;
    clear(box);
    box.appendChild(h("div", { class: "proj-menu-hd", text: "切换项目（一套部署 · 多套本体）" }));
    if (!OPS.projects.length) {
      box.appendChild(h("div", {
        class: "proj-menu-hd",
        text: OPS.admin ? "还没有项目 —— 用顶栏 ＋ 建一个" : "需要 admin key 才能列出项目"
      }));
      return;
    }
    OPS.projects.forEach(function (p) {
      var it = h("div", {
        class: "proj-item" + ((OPS.current && OPS.current.name === p.name) ? " is-cur" : "")
      }, [
        h("div", { class: "proj-item-name", text: p.name }),
        h("div", { class: "proj-item-meta", text: projectMetaShort(p) })
      ]);
      it.addEventListener("click", function () { switchProject(p.name); });
      box.appendChild(it);
    });
  }

  function projectMenuOpen() {
    var m = byId("proj-menu");
    return !!(m && m.classList.contains("is-open"));
  }
  function openProjectMenu() {
    var m = byId("proj-menu");
    if (m) m.classList.add("is-open");
  }
  function closeProjectMenu() {
    var m = byId("proj-menu");
    if (m) m.classList.remove("is-open");
  }
  function toggleProjectMenu() {
    if (projectMenuOpen()) { closeProjectMenu(); return; }
    renderProjectMenu();
    openProjectMenu();
  }

  /* ⭐ AC-d2 · 切项目 ⇒ 重拉 catalog（带 ?project=）⇒ 重绘本体图。
     ⛔ 不整页刷新（那是另一套状态机，且会丢掉口令以外的所有上下文）。 */
  function reloadCatalog(name, done) {
    var pick = byId("proj-pick");
    if (pick) pick.disabled = true;
    return fetchCatalog(function (n, page) {
      setText("ops-hint", "正在读取 " + name + " 的目录（" + n + " 条 · 第 " + page + " 页）…");
    }, name).then(function (out) {
      S.items = out.items;
      S.counts = out.counts;
      OPS.current = findProject(name) || OPS.current
        || { name: name, label: null, active_version: null, frozen: false, entity_types: 0, link_types: 0 };
      buildModel();
      renderAll();
      applyOpsGate();
      if (done) done();
    });
  }

  function switchProject(name) {
    closeProjectMenu();
    if (!OPS.admin) { showBanner("err", "需要 admin key：切项目要带 `?project=`，而那只对 admin 生效。"); return; }
    if (OPS.current && OPS.current.name === name) return;
    setOpsHint("正在切换到 " + name + " …");
    reloadCatalog(name, function () {
      setOpsHint("");
      showBanner("ok", "已切到项目 " + name + "：本体图已重绘（" + S.classes.length + " 个类）");
    }).catch(function (e) {
      var pick = byId("proj-pick");
      if (pick) pick.disabled = false;
      if (e && e.unauthorized) { showAuthInvalid(); applyOpsGate(); return; }
      if (e && e.status === 404) {
        OPS.reason = "not_ready";
        applyOpsGate();
        showBanner("err", "服务端未就绪：切换项目需要的读端点不在（404）。");
        return;
      }
      if (e && e.status === 503) {
        showBanner("err", "项目 " + name + " 还没有生效版本（" + (e.code || "503")
          + "）—— 先上传构建包并激活，再切过来。");
        return;
      }
      showBanner("err", "切换到 " + name + " 失败：" + errText(e));
    });
  }

  /* 读端点带项目（admin 在多项目模式下**必须**带：否则 403 key.admin_has_no_project）。
     `sep` 是 `?` 或 `&`（`/api/describe?name=…` 已经有参数了）。 */
  function projectArg(sep) {
    return (OPS.admin && OPS.current && OPS.current.name)
      ? (sep + "project=" + encodeURIComponent(OPS.current.name)) : "";
  }
  function projectQs() { return projectArg("?"); }

  /* ══════════════════════════════════════════════════════════════
     向导弹窗（JS 构建：index.html 里没有一行 `style=`，CSP 才能生效）
     ══════════════════════════════════════════════════════════════ */

  function ensureWizard() {
    var m = byId("wiz-modal");
    if (m) return m;
    m = h("div", { class: "modal", id: "wiz-modal" }, [
      h("div", { class: "modal-card" }, [
        h("div", { class: "modal-head" }, [
          h("h2", { class: "modal-title", id: "wiz-title", text: "新建项目" }),
          h("div", { class: "modal-sub", id: "wiz-sub" })
        ]),
        h("div", { class: "wiz-steps", id: "wiz-steps" }),
        h("div", { class: "modal-body", id: "wiz-body" }),
        h("div", { class: "modal-foot", id: "wiz-foot" })
      ])
    ]);
    document.body.appendChild(m);
    /* ⭐ 只能通过「取消」关闭：遮罩点击 / ESC 都不关（样机 v2 的第 1 条）。
       这里不绑 click 处理，等于点了没反应 —— 比「点了会关」诚实。 */
    return m;
  }

  function openWizard(startStep) {
    if (!OPS.admin) {
      showBanner("err", "需要 admin key：建项目 / 上传 / 解析 / 激活都要求 admin。");
      return;
    }
    /* 每次打开都是**干净**的一次向导：上一次的包 id / 解析结果 / DSN 结论不能漏进来
       （否则「取消后再点上传」会看到上一轮的回执，甚至带着旧 bundleId 去激活）。 */
    W.open = true;
    W.startStep = startStep;
    W.step = startStep;
    W.name = "";
    W.label = "";
    W.keyLabel = "";
    W.key = "";
    W.created = false;
    W.err = "";
    W.uploaded = null;
    W.uploading = false;
    W.uploadError = "";
    W.bundleId = "";
    W.requiresDb = false;
    W.dsn = "";
    W.dbSaving = false;
    W.dbTested = false;
    W.dbOk = false;
    W.dbResult = null;
    W.dbError = "";
    W.imported = null;
    W.importing = false;
    W.importError = "";
    W.activating = false;
    W.confirmActivate = false;
    W.activated = null;
    W.activateError = "";
    if (startStep >= 2) {
      /* 从顶栏「上传构建包」进来：项目已经有了（OPS.current） */
      W.name = (OPS.current && OPS.current.name) || "";
      W.created = true;
    }
    var m = ensureWizard();
    m.classList.add("is-open");
    renderWizard();
  }

  function closeWizard(force) {
    if (!force) {
      var mid = W.created || !!W.bundleId || W.step > W.startStep;
      if (mid && !W.activated) {
        var yes = window.confirm("确定取消？已填的内容会丢掉，项目/包可能需要重来。");
        if (!yes) return;
      }
    }
    W.open = false;
    var m = byId("wiz-modal");
    if (m) m.classList.remove("is-open");
    if (W.activated) refreshProjectsQuiet();
  }

  function refreshProjectsQuiet() {
    return apiGet(API.projects).then(function (v) {
      if (v && Array.isArray(v.projects)) {
        OPS.projects = v.projects;
        if (OPS.current) OPS.current = findProject(OPS.current.name) || OPS.current;
        renderProjectPick();
      }
      return OPS.projects;
    }, function () { return null; });
  }

  function renderWizard() {
    var title = byId("wiz-title"), sub = byId("wiz-sub");
    if (title) title.textContent = WIZ_TITLES[W.step] || "构建包";
    if (sub) sub.textContent = WIZ_SUBS[W.step] || "";

    /* 步骤条：< 当前步 ⇒ done；= ⇒ now */
    var st = byId("wiz-steps");
    if (st) {
      clear(st);
      WIZ_STEPS.forEach(function (s) {
        /* 包不要 DSN 时，第 3 步是**跳过**的（已上传 ⇒ 标成 done，别让用户以为漏了一步） */
        var skipped = (s.n === 3) && !!W.bundleId && !W.requiresDb;
        var done = s.n < W.step || skipped;
        var cls = "wiz-step" + (done ? " is-done" : (s.n === W.step ? " is-now" : ""));
        st.appendChild(h("div", { class: cls }, [
          h("i", { text: done ? "✓" : String(s.n) }),
          h("span", { text: s.t + (skipped ? "（跳过）" : "") })
        ]));
      });
    }

    var body = byId("wiz-body");
    if (body) {
      clear(body);
      if (W.step === 1) body.appendChild(wizStepCreate());
      else if (W.step === 2) body.appendChild(wizStepUpload());
      else if (W.step === 3) body.appendChild(wizStepDsn());
      else if (W.step === 4) body.appendChild(wizStepImport());
      else body.appendChild(wizStepActivate());
    }

    var foot = byId("wiz-foot");
    if (foot) {
      clear(foot);
      var cancel = h("button", { class: "btn btn-ghost", text: W.activated ? "关闭" : "取消" });
      cancel.addEventListener("click", function () { closeWizard(!!W.activated); });
      foot.appendChild(cancel);
      foot.appendChild(h("div", { class: "spacer" }));

      if (W.step > 2 && !W.activated) {
        var prev = h("button", { class: "btn", text: "上一步" });
        prev.addEventListener("click", function () { W.step = W.step - 1; renderWizard(); });
        foot.appendChild(prev);
      }
      var p = wizPrimary();
      var next = h("button", { class: "btn btn-primary", id: "wiz-next", text: p.text });
      next.disabled = !!p.disabled;
      next.addEventListener("click", p.fn);
      foot.appendChild(next);
    }
  }

  /* footer 的主按钮：文字 / 是否禁用 / 点了做什么 */
  function wizPrimary() {
    if (W.step === 1) {
      return { text: "创建并生成 key", disabled: false, fn: doCreate };
    }
    if (W.step === 2) {
      return { text: "下一步", disabled: !W.bundleId, fn: function () { W.step = W.requiresDb ? 3 : 4; renderWizard(); } };
    }
    if (W.step === 3) {
      /* 不在这里禁用：DSN 没测通也可以去第 4 步看，但**解析按钮**是禁用的（AC-d5 的门在那里） */
      return { text: "下一步", disabled: false, fn: function () { W.step = 4; renderWizard(); } };
    }
    if (W.step === 4) {
      return { text: "下一步", disabled: !W.imported, fn: function () { W.step = 5; renderWizard(); } };
    }
    if (W.activated) {
      return { text: "完成", disabled: false, fn: function () { closeWizard(true); } };
    }
    return { text: "完成", disabled: true, fn: function () {} };
  }

  function field(label, id, tip, value, type) {
    var input = h("input", { class: "fld-input", id: id, type: type || "text", autocomplete: "off", spellcheck: "false" });
    input.value = value || "";
    var box = h("label", { class: "fld" }, [h("span", { class: "fld-label", text: label }), input]);
    if (tip) box.appendChild(h("div", { class: "fld-tip", text: tip }));
    return box;
  }

  function alertBox(kind, text) {
    return h("div", { class: "alert alert-" + kind, text: text });
  }

  function keyBox(key, label, note) {
    var box = h("div", { class: "keybox" });
    var row = h("div", { class: "keyrow" }, [h("div", { class: "keyval", id: "wiz-key", text: key })]);
    var cp = h("button", { class: "btn btn-sm", text: "复制" });
    cp.addEventListener("click", function () { copyText(key, cp); });
    row.appendChild(cp);
    box.appendChild(row);
    box.appendChild(h("div", { class: "keybox-w", text: "给「" + (label || "未命名") + "」用 · 一项目一把 · 要换就 rotate" }));
    if (note) box.appendChild(h("div", { class: "auditnote", text: note }));
    return box;
  }

  function copyText(text, btn) {
    function done() {
      if (!btn) return;
      var old = btn.textContent;
      btn.textContent = "已复制 ✓";
      setTimeout(function () { btn.textContent = old; }, 1200);
    }
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(done, fallback);
      return;
    }
    fallback();
    function fallback() {
      try {
        var ta = document.createElement("textarea");
        ta.value = text;
        ta.style.position = "fixed";
        ta.style.opacity = "0";
        document.body.appendChild(ta);
        ta.select();
        document.execCommand("copy");
        document.body.removeChild(ta);
        done();
      } catch (e) {
        if (btn) btn.textContent = "复制失败，请手动选中";
      }
    }
  }

  /* ── 第 1 步：建项目（key_label 必填 —— 空 ⇒ 前端拦住，⛔ 不发请求） ── */
  function wizStepCreate() {
    var w = h("div");
    w.appendChild(field("项目名 *", "wiz-name",
      "小写字母 / 数字 / 连字符 / 下划线，2–63 位。⛔ 建好不可改名。", W.name));
    w.appendChild(field("备注", "wiz-label", "可选，例如：生产资源本体", W.label));
    w.appendChild(field("这把 key 给谁用 *", "wiz-keylabel",
      "一项目一把 key。写清用途，审计才追得到人（必填）。", W.keyLabel));
    if (W.err) w.appendChild(alertBox("err", W.err));
    return w;
  }

  function doCreate() {
    var n = byId("wiz-name"), l = byId("wiz-label"), k = byId("wiz-keylabel");
    W.name = String(n ? n.value : "").trim();
    W.label = String(l ? l.value : "").trim();
    W.keyLabel = String(k ? k.value : "").trim();
    W.err = "";
    if (!/^[a-z0-9][a-z0-9_-]{1,62}$/.test(W.name)) {
      W.err = "项目名不合法：小写字母 / 数字 / 连字符 / 下划线，2–63 位（不能以 - 或 _ 开头）。";
      renderWizard();
      return;
    }
    if (!W.keyLabel) {
      /* ⭐ AC-d3：这里返回 ⇒ 一次 fetch 都不发（探针数的是请求条数） */
      W.err = "「这把 key 给谁用」必填 —— 它是审计归属，不填就不创建。";
      renderWizard();
      return;
    }
    var payload = { project: W.name, key_label: W.keyLabel };
    if (W.label) payload.label = W.label;
    setOpsHint("正在创建项目 " + W.name + " …");
    apiSend("POST", API.projects, JSON.stringify(payload), "application/json").then(function (out) {
      W.created = true;
      W.key = out.key || "";
      W.keyLabel = out.key_label || W.keyLabel;
      W.step = 2;
      setOpsHint("");
      refreshProjectsQuiet().then(function () {
        OPS.current = findProject(W.name) || OPS.current;
        renderProjectPick();
      });
      renderWizard();
    }).catch(function (e) {
      setOpsHint("");
      W.err = errText(e);
      renderWizard();
    });
  }

  /* ── 第 2 步：上传（octet-stream，body = File.arrayBuffer()） ───── */
  function wizStepUpload() {
    var w = h("div");
    if (W.key) {
      w.appendChild(alertBox("ok", "项目 " + W.name + " 已创建。这是它的一次性 key（只显示这一次）："));
      w.appendChild(keyBox(W.key, W.keyLabel, "之后可在顶栏「查看 key」再拿（每次写一条审计）。"));
    } else {
      w.appendChild(alertBox("info", "向项目 " + (W.name || "—") + " 上传构建包。"));
    }
    var file = h("input", { class: "fld-input", id: "wiz-file", type: "file", accept: ".ontokbundle,application/octet-stream" });
    w.appendChild(h("label", { class: "fld" }, [h("span", { class: "fld-label", text: "构建包（.ontokbundle）*" }), file]));
    var up = h("button", { class: "btn btn-primary", id: "wiz-upload", text: W.uploading ? "上传中…" : "选择并上传" });
    up.disabled = !!W.uploading;
    up.addEventListener("click", doUpload);
    w.appendChild(h("div", { class: "btn-row" }, [up]));
    if (W.uploadError) w.appendChild(alertBox("err", W.uploadError));
    if (W.uploaded) {
      w.appendChild(alertBox("ok", "✓ 上传完成：manifest 校验通过、files[] 的 sha256 全部匹配、project 与接收项目一致。"
        + " bundle_id=" + W.bundleId));
      var t = h("table", { class: "tbl" });
      t.appendChild(h("tr", {}, [
        h("th", { text: "项" }), h("th", { text: "值" })
      ]));
      [
        ["体积", num(W.uploaded.bytes) + " 字节"],
        ["整包 sha256", String(W.uploaded.bundle_sha256 || "").slice(0, 16) + "…"],
        ["要不要 DSN", W.requiresDb ? "要（requires_db = true）" : "不要"],
        ["类 / 关系", num((W.uploaded.counts || {}).entity_types) + " 类 / " + num((W.uploaded.counts || {}).link_types) + " 关系"]
      ].forEach(function (r) {
        t.appendChild(h("tr", {}, [h("th", { text: r[0] }), h("td", { class: "mono", text: r[1] })]));
      });
      w.appendChild(t);
    }
    return w;
  }

  function doUpload() {
    var input = byId("wiz-file");
    var f = (input && input.files && input.files[0]) ? input.files[0] : null;
    W.uploadError = "";
    if (!f) { W.uploadError = "先选一个 .ontokbundle 文件。"; renderWizard(); return; }
    W.uploading = true;
    setOpsHint("正在上传 " + (f.name || "构建包") + " …");
    renderWizard();
    readFileBytes(f).then(function (buf) {
      /* ⭐ AC-d4：Content-Type 是 octet-stream，body 是**原始字节**（⛔ 不是 multipart） */
      return apiSend("POST", projBase(W.name) + "/bundles", buf, "application/octet-stream");
    }).then(function (out) {
      W.uploading = false;
      W.uploaded = out || {};
      W.bundleId = String((out && out.bundle_id) || "");
      W.requiresDb = !!(out && out.requires_db);
      setOpsHint("");
      /* ⛔ 不自动跳到下一步：上传的回执（体积 / sha256 / 要不要 DSN / 计数）要留在眼前，
         由用户点「下一步」再走。跳过去等于把刚做完那一步的证据藏了。 */
      renderWizard();
    }).catch(function (e) {
      W.uploading = false;
      setOpsHint("");
      W.uploadError = errText(e);
      renderWizard();
    });
  }

  /* File → 原始字节（ArrayBuffer）。`fetch` 直接接受它当 body。 */
  function readFileBytes(f) {
    if (f && typeof f.arrayBuffer === "function") return f.arrayBuffer();
    return new Promise(function (resolve, reject) {
      var r = new FileReader();
      r.onload = function () { resolve(r.result); };
      r.onerror = function () { reject(new Error("读文件失败")); };
      r.readAsArrayBuffer(f);
    });
  }

  /* ── 第 3 步：DSN（只有 requires_db 才来这一步） ────────────────── */
  function wizStepDsn() {
    var w = h("div");
    w.appendChild(alertBox("warn", "包里 db.requires_dsn = true ⇒ 不配通**不允许解析**（服务端也会拒）。"));
    var sel = h("select", { class: "fld-input", id: "wiz-dialect" }, [
      h("option", { value: "postgresql", text: "PostgreSQL（v1 只支持这一种）" })
    ]);
    w.appendChild(h("label", { class: "fld" }, [h("span", { class: "fld-label", text: "类型" }), sel]));
    w.appendChild(field("DSN *", "wiz-dsn",
      "加密存元数据库。⛔ 保存后不再回显，日志里也没有。", W.dsn, "password"));
    var save = h("button", { class: "btn btn-primary", id: "wiz-dsn-save", text: W.dbSaving ? "连接中…" : "保存并测试连通性" });
    save.disabled = !!W.dbSaving;
    save.addEventListener("click", doSaveAndTest);
    w.appendChild(h("div", { class: "btn-row" }, [save]));
    if (W.dbSaving) w.appendChild(h("div", { class: "fld-tip", text: "正在连一次性探测（只读、只 SELECT version()）…" }));
    if (W.dbTested) {
      if (W.dbOk) {
        var r = W.dbResult || {};
        w.appendChild(alertBox("ok", "✓ 连通性通过：" + num(r.latency_ms) + " ms · "
          + (r.server_version || "PostgreSQL") + " —— 现在可以解析了。"));
      } else {
        var er = (W.dbResult && W.dbResult.error) || null;
        w.appendChild(alertBox("err", "✕ 连通性没通过：" + (er ? (er.code + "：" + er.message) : W.dbError)
          + " —— 解析按钮仍然是灰的。"));
      }
    }
    return w;
  }

  function doSaveAndTest() {
    var d = byId("wiz-dsn");
    W.dsn = String(d ? d.value : "").trim();
    if (!W.dsn) { W.dbError = "先填 DSN。"; renderWizard(); return; }
    W.dbSaving = true;
    W.dbTested = false;
    W.dbOk = false;
    renderWizard();
    setOpsHint("正在保存 DSN 并测试连通性…");
    apiSend("PUT", projBase(W.name) + "/database",
      JSON.stringify({ dsn: W.dsn, dialect: "postgresql" }), "application/json")
      .then(function () {
        return apiSend("POST", projBase(W.name) + "/database/test", null, null);
      })
      .then(function (t) {
        W.dbSaving = false;
        W.dbTested = true;
        /* 测连通**失败也 200**：结论在 `ok` 上，不在状态码上 */
        W.dbResult = t || {};
        W.dbOk = !!(t && t.ok === true);
        W.dbError = W.dbOk ? "" : "服务端说没通过（ok=false）";
        setOpsHint("");
        renderWizard();
      })
      .catch(function (e) {
        W.dbSaving = false;
        W.dbTested = true;
        W.dbOk = false;
        W.dbError = errText(e);
        setOpsHint("");
        renderWizard();
      });
  }

  /* ── 第 4 步：解析（already_imported:true 是**成功**） ─────────── */
  function wizStepImport() {
    var w = h("div");
    w.appendChild(alertBox("info", "单事务：中途失败一个字节都不落。解析成功 ≠ 对外生效（激活才生效）。"));
    if (W.requiresDb && !W.dbOk) {
      w.appendChild(alertBox("warn", "这个包 requires_db = true，而 DSN 还没测通 ⇒ 解析按钮是灰的。先在「配置 DB」测通。"));
    }
    var bar = h("div", { class: "bar" }, [
      h("i", { class: "bar-fill" + (W.imported ? " is-full" : "") })
    ]);
    w.appendChild(bar);
    var btn = h("button", { class: "btn btn-primary", id: "wiz-import", text: W.importing ? "解析中…" : (W.imported ? "再解析一次" : "开始解析") });
    /* ⭐ AC-d5 的那道门：requires_db 且没测通 ⇒ **disabled** */
    btn.disabled = !!W.importing || (W.requiresDb && !W.dbOk);
    btn.addEventListener("click", doImport);
    w.appendChild(h("div", { class: "btn-row" }, [btn]));
    if (W.importing) w.appendChild(h("div", { class: "fld-tip", text: "正在解析入库（单事务）…" }));
    if (W.importError) w.appendChild(alertBox("err", W.importError));
    if (W.imported) {
      var im = W.imported;
      var c = im.counts || {};
      /* ⭐ AC-d6：already_imported 是**幂等成功** —— 同一个分支、同一句「成功」，
         ⛔ 不进错误分支、⛔ 不清状态、⛔ 不禁用下一步 */
      w.appendChild(alertBox("ok", im.already_imported
        ? ("✓ 这个包已经解析过（幂等成功）：沿用版本 v" + im.version + "，没有重复建版本。")
        : ("✓ 解析成功 → 新版本 v" + im.version + "（未冻结，对外还不可见）。")));
      var t = h("table", { class: "tbl" });
      t.appendChild(h("tr", {}, [h("th", { text: "计数" }), h("th", { text: "值" })]));
      [
        ["对象类型", num(c.entity_types)],
        ["关系类型", num(c.link_types)],
        ["属性", num(c.properties)],
        ["动作 / 函数", num(c.action_types) + " / " + num(c.functions)],
        ["概念组", num(c.concept_groups)]
      ].forEach(function (r) {
        t.appendChild(h("tr", {}, [h("th", { text: r[0] }), h("td", { class: "mono", text: String(r[1]) })]));
      });
      w.appendChild(t);
      if (im.warnings && im.warnings.length) {
        w.appendChild(alertBox("warn", "warnings（" + im.warnings.length + " 条）：" + im.warnings.join(" / ")));
      }
    }
    return w;
  }

  function doImport() {
    if (W.requiresDb && !W.dbOk) return;   /* 按钮已经 disabled；这里是第二道保险 */
    W.importing = true;
    W.importError = "";
    renderWizard();
    setOpsHint("正在解析 " + W.bundleId + " …");
    apiSend("POST", projBase(W.name) + "/bundles/" + encodeURIComponent(W.bundleId) + "/import", null, null)
      .then(function (out) {
        W.importing = false;
        W.imported = out || {};
        setOpsHint("");
        renderWizard();
      })
      .catch(function (e) {
        W.importing = false;
        setOpsHint("");
        W.importError = errText(e);
        renderWizard();
      });
  }

  /* ── 第 5 步：激活（二次确认，文案写明「会影响对外服务」） ────── */
  function wizStepActivate() {
    var w = h("div");
    var cur = OPS.current || {};
    var t = h("table", { class: "tbl" });
    [
      ["项目", W.name || "—"],
      ["新版本", W.imported ? ("v" + W.imported.version) : "—"],
      ["当前生效", (cur.active_version === null || cur.active_version === undefined)
        ? "无（还没有生效版本）" : ("v" + cur.active_version)],
      ["冻结", W.imported && W.imported.frozen ? "已冻结" : "未冻结"]
    ].forEach(function (r) {
      t.appendChild(h("tr", {}, [h("th", { text: r[0] }), h("td", { class: "mono", text: r[1] })]));
    });
    w.appendChild(t);

    if (W.activated) {
      w.appendChild(alertBox("ok", "✓ 已激活：项目 " + W.name + " 现在对外服务的是 v" + W.activated.version + "。"));
      w.appendChild(h("div", { class: "auditnote", text: "旧版本仍然冻结可查（回滚是 P1c-3 的事）。" }));
      return w;
    }
    w.appendChild(alertBox("info", "解析 ≠ 生效：点「冻结并激活」才会把项目指针指到这一版。旧版本仍冻结可查。"));
    var ask = h("button", { class: "btn btn-danger", id: "wiz-ask-activate", text: W.activating ? "激活中…" : "冻结并激活" });
    ask.disabled = !!W.activating || !W.imported;
    ask.addEventListener("click", function () { W.confirmActivate = true; renderWizard(); });
    w.appendChild(h("div", { class: "btn-row" }, [ask]));

    if (W.confirmActivate) {
      var box = h("div", { class: "confirm", id: "wiz-confirm" });
      box.appendChild(h("p", { text: "⚠️ 再确认一次：激活会**影响对外服务** —— 数字员工的下一次请求就会读到 v"
        + (W.imported ? W.imported.version : "?") + "。确认冻结并激活？" }));
      var row = h("div", { class: "btn-row" });
      var yes = h("button", { class: "btn btn-danger", id: "wiz-do-activate", text: "确认激活（会影响对外服务）" });
      yes.addEventListener("click", doActivate);
      var no = h("button", { class: "btn btn-ghost", id: "wiz-cancel-activate", text: "再想想" });
      no.addEventListener("click", function () { W.confirmActivate = false; renderWizard(); });
      row.appendChild(yes);
      row.appendChild(no);
      box.appendChild(row);
      w.appendChild(box);
    }
    if (W.activateError) w.appendChild(alertBox("err", W.activateError));
    return w;
  }

  function doActivate() {
    W.confirmActivate = false;
    W.activating = true;
    W.activateError = "";
    renderWizard();
    setOpsHint("正在激活 v…（会改对外服务）");
    apiSend("POST", projBase(W.name) + "/bundles/" + encodeURIComponent(W.bundleId) + "/activate", null, null)
      .then(function (out) {
        W.activating = false;
        W.activated = out || {};
        setOpsHint("");
        renderWizard();
        /* 激活之后**立刻**把图换成新版本（否则用户看到的还是旧的一版） */
        return refreshProjectsQuiet().then(function () {
          return reloadCatalog(W.name, function () {
            showBanner("ok", "项目 " + W.name + " 已激活 v" + (W.activated ? W.activated.version : "?")
              + "：图已刷新为这一版。");
          });
        });
      })
      .catch(function (e) {
        W.activating = false;
        setOpsHint("");
        W.activateError = errText(e);
        renderWizard();
      });
  }

  /* ══════════════════════════════════════════════════════════════
     查看 key（独立弹窗；固定一行「本次查看会记录一条审计」）
     ══════════════════════════════════════════════════════════════ */

  function ensureKeyModal() {
    var m = byId("key-modal");
    if (m) return m;
    m = h("div", { class: "modal", id: "key-modal" }, [
      h("div", { class: "modal-card is-narrow" }, [
        h("div", { class: "modal-head" }, [
          h("h2", { class: "modal-title", text: "查看 key" }),
          h("div", { class: "modal-sub", text: "admin 才能看；每次查看都会写一条审计。" })
        ]),
        h("div", { class: "modal-body", id: "key-body" }),
        h("div", { class: "modal-foot", id: "key-foot" })
      ])
    ]);
    document.body.appendChild(m);
    return m;
  }

  function openKeyModal() {
    if (!OPS.admin) {
      showBanner("err", "需要 admin key：查看 key 也要求 admin（每次查看写一条审计）。");
      return;
    }
    KEYM.open = true;
    KEYM.loading = true;
    KEYM.data = null;
    KEYM.error = "";
    var m = ensureKeyModal();
    m.classList.add("is-open");
    renderKeyModal();
    doRevealKey();
  }

  function closeKeyModal() {
    KEYM.open = false;
    var m = byId("key-modal");
    if (m) m.classList.remove("is-open");
  }

  function renderKeyModal() {
    var body = byId("key-body");
    var foot = byId("key-foot");
    if (!body || !foot) return;
    clear(body);
    clear(foot);

    var name = (OPS.current && OPS.current.name) || "—";
    if (KEYM.loading) {
      body.appendChild(alertBox("info", "正在读取项目 " + name + " 的 key…"));
    } else if (KEYM.error) {
      body.appendChild(alertBox("err", KEYM.error));
    } else if (KEYM.data) {
      body.appendChild(alertBox("info", "项目 " + KEYM.data.project + " · 用途 "
        + (KEYM.data.label || "（没写）")));
      body.appendChild(keyBox(KEYM.data.key, KEYM.data.label,
        "创建于 " + String(KEYM.data.created_at || "—")));
    }
    /* 固定一行（有没有 key 都显示）：这是「可查看」的唯一补偿 */
    body.appendChild(h("div", {
      class: "auditnote",
      id: "key-audit-note",
      text: "本次查看会记录一条审计（action=reveal_key）。"
    }));

    var close = h("button", { class: "btn btn-ghost", text: "关闭" });
    close.addEventListener("click", closeKeyModal);
    foot.appendChild(close);
    foot.appendChild(h("div", { class: "spacer" }));
    var cp = h("button", { class: "btn btn-primary", id: "key-copy", text: "复制" });
    cp.disabled = !(KEYM.data && KEYM.data.key);
    cp.addEventListener("click", function () {
      if (KEYM.data && KEYM.data.key) copyText(KEYM.data.key, cp);
    });
    foot.appendChild(cp);
  }

  function doRevealKey() {
    var name = (OPS.current && OPS.current.name) || "";
    if (!name) {
      KEYM.loading = false;
      KEYM.error = "先选一个项目。";
      renderKeyModal();
      return;
    }
    apiSend("GET", projBase(name) + "/key", null, null).then(function (v) {
      KEYM.loading = false;
      KEYM.data = v || null;
      renderKeyModal();
    }).catch(function (e) {
      KEYM.loading = false;
      if (e && e.status === 404) {
        KEYM.error = "服务端未就绪：查看 key 的接口不在（404）。";
      } else if (e && e.status === 403) {
        KEYM.error = "需要 admin key：" + (e.code || "403") + "（当前口令没有写面权限）。";
      } else if (e && e.unauthorized) {
        KEYM.error = "口令无效或已过期（401）—— 重新输入口令后再试。";
      } else {
        KEYM.error = "读取 key 失败：" + errText(e);
      }
      renderKeyModal();
    });
  }

  /* ══════════════════════════════════════════════════════════════
     装配
     ══════════════════════════════════════════════════════════════ */

  function renderAll() {
    var h0 = S.health || {};
    /* 加载态期间 tab 是 disabled 的（壳里没有数据可切）；数据到了才打开。 */
    Array.prototype.slice.call(document.querySelectorAll("#tabs button")).forEach(function (b) {
      b.disabled = false;
    });
    /* 顶部（项目名/版本由顶栏那个下拉显示：OPS.current 优先，其次 health） */
    renderProjectPick();
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

    /* ⭐ REQ-159 P1c-2 · 顶栏入口（默认全 disabled，admin 判定通过后才打开） */
    byId("proj-pick").addEventListener("click", function (e) {
      e.stopPropagation();
      if (!OPS.admin) return;
      toggleProjectMenu();
    });
    byId("proj-add").addEventListener("click", function () { openWizard(1); });
    byId("bundle-open").addEventListener("click", function () {
      if (!OPS.admin) return;
      if (!OPS.current) { showBanner("err", "先建一个项目（顶栏 ＋），再上传构建包。"); return; }
      openWizard(2);
    });
    byId("key-view").addEventListener("click", openKeyModal);

    /* 点别处收起项目菜单（菜单本身的事件 stopPropagation） */
    document.addEventListener("click", function (e) {
      if (e && e.target && e.target.closest && e.target.closest(".projsel")) return;
      closeProjectMenu();
    });
    var menu = byId("proj-menu");
    if (menu) menu.addEventListener("click", function (e) { e.stopPropagation(); });

    /* ⛔ 向导/查看 key **只能通过「取消」或「关闭」退出**：ESC 明确不生效
       （与样机 v2 同一口径 —— 用户点过一次才发现「咦，填的都丢了」）。 */
    document.addEventListener("keydown", function (e) {
      if (e.key !== "Escape" && e.key !== "Esc") return;
      if (!W.open && !KEYM.open) return;
      e.preventDefault();
      if (W.open) {
        var sub = byId("wiz-sub");
        if (sub) sub.textContent = "按「取消」关闭这个向导（ESC 与点遮罩都不生效）。";
      }
    });

    applyOpsGate();
    renderProjectPick();

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
