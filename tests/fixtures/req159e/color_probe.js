#!/usr/bin/env node
// ⭐ REQ-159 **P1c-2b** · Studio 节点着色「两种模式」判据（Node + 假 DOM；本机没有 headless 浏览器）。
//
// 它把 `src/studio/studio.js` **原样**（不复制、不改写）放进一个最小 DOM 桩里跑，
// 静态节点由 `src/studio/index.html` 里的 id 生成 —— 于是验的是**线上那一份代码**。
//
// | 判据 | 验的是什么 |
// | --- | --- |
// | AC-e1 | 顶栏「着色」两项；默认「可查性」；切到中心度后 sessionStorage 记住；URL 一个字节没动 |
// | AC-e2 | 切到中心度 ⇒ 节点色**真的变了**（≥2 种颜色），且图例换成中心度那套 |
// | AC-e3 | 已知 deg 分布（20 个类，度数 0..9 各两个）⇒ 四档边界 = P50/P80/P95，**边界值 exact** |
// | AC-e4 | 图例跟着模式变：中心度图例里 ⛔ 不许出现「需取数 / API 源 / 库源」；可查性图例里 ⛔ 不许出现「枢纽」 |
// | AC-e5 | 切换⛔ 不重建图：reload 0 次、g-root 0 次新建、node-g 0 次新建、节点对象全等、cx/cy 不变 |
// | AC-e7 | 模式 A 的三态三色**逐字未变**（可查 / 需取数 / 不可查 ⇒ 蓝 / 琥珀 / 灰） |
//
// 跑法：node tests/fixtures/req159e/color_probe.js
// 退出码：0 = 全过；1 = 有判据失败（可直接当判据用）。

"use strict";

const fs = require("fs");
const path = require("path");
const vm = require("vm");

const ROOT = path.resolve(__dirname, "..", "..", "..");
const SCRIPT = fs.readFileSync(path.join(ROOT, "src", "studio", "studio.js"), "utf8");
const HTML = fs.readFileSync(path.join(ROOT, "src", "studio", "index.html"), "utf8");

const TOKEN = "FAKE-REQ159E-ADMIN-0123456789abcdef";
const TOKEN_KEY = "ontok_studio_token";
const COLOR_KEY = "ontok_studio_color_mode";

/* 模式 A 的三个色值 + 模式 B 的四档色值：**照抄** studio.js 的常量表。
   ⚠️ 这里再写一遍是**故意的**：判据不能从被测代码里推导（那样改错也对上）。 */
const TONE_API = "#e6b455";
const TONE_OK = "#6ec8ff";
const TONE_NONE = "#5d6a80";
const TIER = {
  hub: { label: "枢纽", color: "#ef7a8a" },
  core: { label: "中心", color: "#e6b455" },
  mid: { label: "一般", color: "#6ec8ff" },
  leaf: { label: "边缘", color: "#5d6a80" }
};

// ── 断言器 ────────────────────────────────────────────────────
let pass = 0;
const failures = [];
function ok(cond, what, detail) {
  if (cond) {
    pass += 1;
    console.log("  ✓ " + what);
  } else {
    failures.push(what + (detail ? ("\n      " + detail) : ""));
    console.log("  ✗ " + what + (detail ? ("\n      " + detail) : ""));
  }
  return !!cond;
}
function section(title) { console.log("\n" + title); }

// ── 假 DOM ────────────────────────────────────────────────────
function createDom() {
  const registry = new Map();
  /* AC-e5 的计数器：g-root 每次重建 +1；node-g 每建一个 +1。
     「布局跑没跑」= 「g-root 有没有被换掉」——`simulate()` 在 studio.js 里
     只有 `buildGraph()` 一个调用点，而 buildGraph 必然先 clear(svg) 再建一个新的 g-root。 */
  const stats = { nodeG: 0, gRoot: 0 };

  function makeNode(tag, ns) {
    const node = {
      nodeType: 1,
      tagName: String(tag || "div").toUpperCase(),
      namespaceURI: ns || null,
      id: "",
      _class: "",
      _attrs: {},
      _text: "",
      value: "",
      disabled: false,
      hidden: false,
      checked: false,
      files: null,
      title: "",
      style: {},
      children: [],
      parentNode: null,
      _listeners: {},
      appendChild(c) {
        if (c && c._isFragment) {
          c.children.slice().forEach((k) => node.appendChild(k));
          c.children = [];
          return c;
        }
        node.children.push(c);
        c.parentNode = node;
        if (c.id) registry.set(c.id, c);
        return c;
      },
      insertBefore(c) { return node.appendChild(c); },
      removeChild(c) {
        node.children = node.children.filter((x) => x !== c);
        c.parentNode = null;
        return c;
      },
      addEventListener(t, fn) { (node._listeners[t] = node._listeners[t] || []).push(fn); },
      removeEventListener() {},
      setAttribute(k, v) {
        node._attrs[k] = String(v);
        if (k === "id") {
          node.id = String(v);
          registry.set(node.id, node);
          if (node.id === "g-root") stats.gRoot += 1;
        }
        if (k === "class") {
          node.className = String(v);
          if (String(v).split(/\s+/).includes("node-g")) stats.nodeG += 1;
        }
        if (k === "disabled") node.disabled = true;
      },
      getAttribute(k) {
        if (k === "class") return node.className;
        return Object.prototype.hasOwnProperty.call(node._attrs, k) ? node._attrs[k] : null;
      },
      closest(sel) {
        let cur = node;
        while (cur) {
          if (sel === ".node-g" && String(cur.className || "").split(/\s+/).includes("node-g")) return cur;
          if (sel === ".projsel" && String(cur.className || "").split(/\s+/).includes("projsel")) return cur;
          cur = cur.parentNode;
        }
        return null;
      },
      getBoundingClientRect() { return { left: 0, top: 0, width: 1200, height: 800, right: 1200, bottom: 800 }; },
      getBBox() { return { x: -400, y: -300, width: 800, height: 600 }; },
      setPointerCapture() {}, releasePointerCapture() {},
      select() {}, focus() {}, blur() {}, scrollIntoView() {},
      querySelectorAll() { return []; },
      click() {
        if (node.disabled) return false;
        dispatch(node, "click", {});
        return true;
      }
    };
    Object.defineProperty(node, "className", {
      get() { return node._class; },
      set(v) { node._class = String(v || ""); node._attrs.class = node._class; }
    });
    const classList = {
      add() {
        const s = new Set(String(node.className).split(/\s+/).filter(Boolean));
        for (const a of arguments) s.add(a);
        node.className = [...s].join(" ");
      },
      remove() {
        const s = new Set(String(node.className).split(/\s+/).filter(Boolean));
        for (const a of arguments) s.delete(a);
        node.className = [...s].join(" ");
      },
      toggle(name, force) {
        const has = classList.contains(name);
        const want = force === undefined ? !has : !!force;
        if (want) classList.add(name); else classList.remove(name);
        return want;
      },
      contains(name) { return String(node.className).split(/\s+/).includes(name); }
    };
    Object.defineProperty(node, "classList", { get() { return classList; } });
    Object.defineProperty(node, "textContent", {
      get() { return node._text + node.children.map((c) => c.textContent || "").join(""); },
      set(v) { node._text = (v === undefined || v === null) ? "" : String(v); node.children = []; }
    });
    Object.defineProperty(node, "innerHTML", {
      get() { return node._text; },
      set() { throw new Error("probe: innerHTML 被赋值（Studio 的纪律是 createElement + textContent）"); }
    });
    Object.defineProperty(node, "firstChild", {
      get() { return node.children.length ? node.children[0] : null; }
    });
    return node;
  }

  function dispatch(node, type, extra) {
    const ev = Object.assign({
      type,
      target: node,
      preventDefault() {}, stopPropagation() {},
      clientX: 0, clientY: 0, button: 0, buttons: 0
    }, extra || {});
    (node._listeners[type] || []).slice().forEach((fn) => fn(ev));
    return ev;
  }

  // 静态节点：从 index.html 里抽 <tag ... id="...">
  const re = /<([a-zA-Z][\w-]*)\b([^>]*\bid="([^"]+)"[^>]*)>/g;
  let m;
  while ((m = re.exec(HTML)) !== null) {
    const tag = m[1];
    const attrs = m[2];
    const id = m[3];
    const node = makeNode(tag);
    node.id = id;
    node._attrs.id = id;
    if (/\bdisabled\b/.test(attrs)) node.disabled = true;
    registry.set(id, node);
  }

  const document = {
    readyState: "complete",
    hidden: false,
    _listeners: {},
    getElementById(id) { return registry.has(id) ? registry.get(id) : null; },
    createElement(tag) { return makeNode(tag); },
    createElementNS(ns, tag) { return makeNode(tag, ns); },
    createTextNode(t) {
      const n = makeNode("#text");
      n._text = String(t);
      n._isText = true;
      return n;
    },
    createDocumentFragment() {
      const n = makeNode("#fragment");
      n._isFragment = true;
      return n;
    },
    querySelectorAll() { return []; },
    addEventListener(t, fn) { (document._listeners[t] = document._listeners[t] || []).push(fn); }
  };
  document.body = makeNode("body");

  return { makeNode, document, registry, stats, dispatch };
}

function makeEnv(opts) {
  opts = opts || {};
  const dom = createDom();
  const document = dom.document;
  const requests = [];
  const state = { handler: opts.handler, reloads: 0, urlWrites: 0, raf: 0 };

  function fetchStub(url, init) {
    init = init || {};
    const method = String(init.method || "GET").toUpperCase();
    const headers = init.headers || {};
    const req = {
      method,
      url: String(url),
      headers,
      body: init.body,
      contentType: headers["Content-Type"] || headers["content-type"] || ""
    };
    requests.push(req);
    let reply;
    try {
      reply = state.handler(req);
    } catch (e) {
      return Promise.reject(e);
    }
    if (reply === undefined || reply === null) {
      return Promise.reject(new TypeError("probe: network disabled for " + method + " " + url));
    }
    if (typeof reply.then === "function") return reply;
    return Promise.resolve({
      ok: reply.status >= 200 && reply.status < 300,
      status: reply.status,
      headers: { get: () => "application/json" },
      text: () => Promise.resolve(JSON.stringify(reply.body === undefined ? {} : reply.body))
    });
  }

  const session = new Map();
  const sandbox = {
    console,
    setTimeout,
    clearTimeout,
    Promise,
    JSON,
    Math,
    Date,
    String,
    Number,
    Array,
    Object,
    encodeURIComponent,
    decodeURIComponent,
    isNaN,
    parseInt,
    parseFloat,
    RegExp,
    Error,
    TypeError,
    document,
    fetch: fetchStub,
    navigator: { clipboard: { writeText: () => Promise.resolve() } },
    sessionStorage: {
      getItem: (k) => (session.has(k) ? session.get(k) : null),
      setItem: (k, v) => session.set(k, String(v)),
      removeItem: (k) => session.delete(k),
      _map: session
    },
    _requests: requests,
    _state: state,
    _stats: dom.stats,
    _listeners: {}
  };
  sandbox.window = sandbox;
  sandbox.globalThis = sandbox;
  sandbox.window.innerWidth = 1280;
  sandbox.window.innerHeight = 800;
  sandbox.window.requestAnimationFrame = function () {
    state.raf += 1;   // 定帧：图谱不跑动画（也用来证明「切换没有触发布局/物理」）
    return 0;
  };
  /* AC-e1：模式**不许**进 URL —— search/hash 的 setter 直接抛；href 只读快照。 */
  sandbox.window.location = {
    href: "http://127.0.0.1:18999/studio",
    pathname: "/studio",
    get search() { return ""; },
    set search(v) { state.urlWrites += 1; throw new Error("probe: 不许把着色模式写进 URL"); },
    get hash() { return ""; },
    set hash(v) { state.urlWrites += 1; throw new Error("probe: 不许把着色模式写进 URL"); },
    reload() {
      state.reloads += 1;
      throw new Error("probe: 整页刷新（切着色模式不该刷新）");
    }
  };
  sandbox.window.confirm = function () { return true; };
  sandbox.window.addEventListener = function (t, fn) {
    (sandbox._listeners[t] = sandbox._listeners[t] || []).push(fn);
  };
  if (opts.token !== null) session.set(TOKEN_KEY, opts.token || TOKEN);
  if (opts.colorMode) session.set(COLOR_KEY, opts.colorMode);

  vm.createContext(sandbox);
  return { sandbox, document, registry: dom.registry, requests, state, dispatch: dom.dispatch, session, stats: dom.stats };
}

// ── 小工具 ─────────────────────────────────────────────────────
const tick = (ms) => new Promise((r) => setTimeout(r, ms || 25));

function byId(env, id) { return env.document.getElementById(id); }
function text(env, id) { const n = byId(env, id); return n ? n.textContent : ""; }
function classesOf(node) { return String(node.className || "").split(/\s+/).filter(Boolean); }
function hasClass(node, c) { return !!node && classesOf(node).includes(c); }
function walk(node, fn) {
  if (!node) return;
  fn(node);
  (node.children || []).forEach((c) => walk(c, fn));
}

/* 图上每个节点：key → { ring 色, halo 渐变, cx, 元素对象 } */
function nodePaint(env) {
  const out = new Map();
  walk(byId(env, "graph"), (el) => {
    if (!hasClass(el, "node-g")) return;
    const key = el.getAttribute("data-key");
    let ring = null, halo = null, sphere = null, orbit = null, rim = null;
    (el.children || []).forEach((c) => {
      if (hasClass(c, "node-ring")) ring = c;
      if (hasClass(c, "node-halo")) halo = c;
      if (hasClass(c, "node-sphere")) sphere = c;
      if (hasClass(c, "node-orbit")) orbit = c;
      if (hasClass(c, "node-rim")) rim = c;
    });
    out.set(key, {
      el,
      ring,
      halo,
      sphere,
      orbit,
      rim,
      tone: ring ? ring.getAttribute("stroke") : null,
      glow: halo ? halo.getAttribute("fill") : null,
      sphereFill: sphere ? sphere.getAttribute("fill") : null,
      cx: halo ? halo.getAttribute("cx") : null,
      cy: halo ? halo.getAttribute("cy") : null
    });
  });
  return out;
}
function tonesOf(paint) {
  const set = new Set();
  paint.forEach((p) => { if (p.tone) set.add(p.tone); });
  return [...set].sort();
}

// ── 目录响应 ───────────────────────────────────────────────────
function catBody(project, classes, relations, opts) {
  opts = opts || {};
  const items = [];
  classes.forEach((c) => {
    const item = {
      kind: "class", name: c, display_name: c + " 类", iri: "https://ontok.local/" + project + "#" + c,
      property_count: 2, relation_count: (relations || []).length
    };
    if (opts.apiNames && opts.apiNames.indexOf(c) >= 0) {
      item.access = { kind: "api", pushdown_fields: ["id"] };
    }
    items.push(item);
  });
  (relations || []).forEach((r) => {
    items.push({
      kind: "relation", name: r[0] + "__" + r[1], from: r[0], to: r[1],
      evidence_level: "unverified"
    });
  });
  return {
    project: project, version: 3, page: 1, page_size: 200, has_more: false,
    counts: { classes: classes.length, properties: classes.length * 2, relations: (relations || []).length },
    items
  };
}

function healthHandler(mode) {
  return function (req) {
    const u = req.url;
    if (req.method === "GET" && u === "/api/health") {
      return { status: 200, body: { project: "probe-proj", ontology_version: 3, mode: mode } };
    }
    if (req.method === "GET" && u === "/api/projects") {
      return { status: 403, body: { code: "key.not_admin", message: "probe: 读图不需要 admin" } };
    }
    return null;
  };
}

async function bootEnv(opts) {
  const env = makeEnv(opts);
  vm.runInContext(SCRIPT, env.sandbox, { filename: "studio.js" });
  await tick(60);
  return env;
}

// ══════════════════════════════════════════════════════════════
// S1：默认「可查性」= 现状（AC-e1 / AC-e7）
// ══════════════════════════════════════════════════════════════
async function scenarioDefaultVerifiability() {
  section("[S1] 默认「可查性」：三态三色未变、图例是那套、选择还没写进 sessionStorage");

  /* ① 查询模式：API 源（琥珀）+ 库源（蓝） */
  const envQuery = await bootEnv({
    token: TOKEN,
    handler(req) {
      const base = healthHandler("query")(req);
      if (base) return base;
      if (req.method === "GET" && req.url.indexOf("/api/catalog") === 0) {
        return {
          status: 200,
          body: catBody("probe-proj", ["api_a", "db_b"], [["api_a", "db_b"]], { apiNames: ["api_a"] })
        };
      }
      return null;
    }
  });

  const sel = byId(envQuery, "color-mode");
  ok(!!sel, "AC-e1 · 顶栏有「着色」下拉（#color-mode）");
  ok(sel && sel.value === "verifiability",
    "AC-e1 · 默认是「可查性」（verifiability）", sel ? "实际 " + sel.value : "没有下拉");
  ok(envQuery.session.get(COLOR_KEY) === undefined,
    "AC-e1 · 没切之前不写 sessionStorage（默认值不落盘）",
    "实际 " + JSON.stringify(envQuery.session.get(COLOR_KEY)));
  ok(envQuery.state.urlWrites === 0, "AC-e1 · 没有一次 URL 写入");
  ok(envQuery.sandbox.window.location.href === "http://127.0.0.1:18999/studio",
    "AC-e1 · location.href 一个字节没变", envQuery.sandbox.window.location.href);
  ok(envQuery.requests.every((r) => r.url.indexOf("color") < 0),
    "AC-e1 · 没有任何请求带 color 参数");

  const paintQ = nodePaint(envQuery);
  ok(paintQ.size === 2, "AC-e7 · 图上有 2 个节点", String(paintQ.size));
  ok(paintQ.get("api_a") && paintQ.get("api_a").tone === TONE_API,
    "AC-e7 · API 源 = 琥珀 #e6b455（逐字未变）",
    paintQ.get("api_a") ? paintQ.get("api_a").tone : "缺节点");
  ok(paintQ.get("db_b") && paintQ.get("db_b").tone === TONE_OK,
    "AC-e7 · 库源 + 查询模式 = 蓝 #6ec8ff（逐字未变）",
    paintQ.get("db_b") ? paintQ.get("db_b").tone : "缺节点");
  ok(paintQ.get("api_a") && paintQ.get("api_a").glow === "url(#glow-warn)",
    "AC-e7 · API 源的泛光仍是 glow-warn", paintQ.get("api_a") ? paintQ.get("api_a").glow : "");
  ok(paintQ.get("db_b") && paintQ.get("db_b").glow === "url(#glow-ok)",
    "AC-e7 · 可查的泛光仍是 glow-ok", paintQ.get("db_b") ? paintQ.get("db_b").glow : "");

  const legendQ = text(envQuery, "legend");
  ok(legendQ.indexOf("API 源 1 类") >= 0 && legendQ.indexOf("库源 1 类") >= 0,
    "AC-e7 · 模式 A 图例仍是「API 源 / 库源」那两句", legendQ);
  ok(legendQ.indexOf("枢纽") < 0 && legendQ.indexOf("中心度") < 0,
    "AC-e4 · 可查性图例里没有「枢纽 / 中心度」", legendQ);

  /* ② 本体模式：库源 = 灰（不可查） */
  const envOnto = await bootEnv({
    token: TOKEN,
    handler(req) {
      const base = healthHandler("ontology-only")(req);
      if (base) return base;
      if (req.method === "GET" && req.url.indexOf("/api/catalog") === 0) {
        return { status: 200, body: catBody("probe-proj", ["db_c"], []) };
      }
      return null;
    }
  });
  const paintO = nodePaint(envOnto);
  ok(paintO.get("db_c") && paintO.get("db_c").tone === TONE_NONE,
    "AC-e7 · 本体模式（ontology-only）的库源 = 灰 #5d6a80（逐字未变）",
    paintO.get("db_c") ? paintO.get("db_c").tone : "缺节点");
  ok(paintO.get("db_c") && paintO.get("db_c").glow === "url(#glow-dim)",
    "AC-e7 · 不可查的泛光仍是 glow-dim", paintO.get("db_c") ? paintO.get("db_c").glow : "");
  return envQuery;
}

// ══════════════════════════════════════════════════════════════
// S2：切到「中心度」⇒ 颜色变、图例变、图不重建（AC-e2 / AC-e4 / AC-e5）
// ══════════════════════════════════════════════════════════════
async function scenarioSwitchToDegree() {
  section("[S2] 切到「中心度」：颜色变、图例跟着变、⛔ 不重建图、切回来能还原");

  const CLASSES = ["a", "b", "c", "d", "e"];
  const EDGES = [["a", "b"], ["a", "c"], ["a", "d"]];
  /* deg：a=3 b=1 c=1 d=1 e=0 ⇒ 排序 [0,1,1,1,3] ⇒ P50=1 P80=1 P95=3 */
  const env = await bootEnv({
    token: TOKEN,
    handler(req) {
      const base = healthHandler("ontology-only")(req);
      if (base) return base;
      if (req.method === "GET" && req.url.indexOf("/api/catalog") === 0) {
        return { status: 200, body: catBody("probe-proj", CLASSES, EDGES) };
      }
      return null;
    }
  });

  const before = nodePaint(env);
  const beforeTones = [...before.entries()].map(([k, p]) => k + "=" + p.tone).sort();
  const beforeV = [...before.values()];
  const beforeNodeG = env.stats.nodeG;
  const beforeGRoot = env.stats.gRoot;
  const beforeRaf = env.state.raf;
  const beforeReloads = env.state.reloads;
  const legendBefore = text(env, "legend");

  ok(beforeTones.length === 5 && tonesOf(before).length === 1,
    "AC-e2 前置：模式 A 下五个节点**同一种颜色**（本体模式 ⇒ 全灰，正是用户抱怨的那一版）",
    tonesOf(before).join(","));

  /* 切模式：只动 <select> 的值 + 派发 change（与真人操作同一条路径） */
  const sel = byId(env, "color-mode");
  sel.value = "degree";
  env.dispatch(sel, "change", {});
  await tick(10);

  /* ── AC-e5：不重建图 ─────────────────────────────────────── */
  const after = nodePaint(env);
  const sameObjects = [...before.entries()].every(([k, p]) => after.get(k) && after.get(k).el === p.el);
  const cxMoved = [...before.entries()].filter(([k, p]) => {
    const q = after.get(k);
    return !q || q.cx !== p.cx || q.cy !== p.cy;
  }).length;
  const svgChildSame = byId(env, "graph").children[0] === (beforeV[0] ? beforeV[0].el.parentNode.parentNode : null);
  console.log("  · AC-e5 探针：reload=" + (env.state.reloads - beforeReloads)
    + " · g-root 新建=" + (env.stats.gRoot - beforeGRoot)
    + " · node-g 新建=" + (env.stats.nodeG - beforeNodeG)
    + " · requestAnimationFrame=" + (env.state.raf - beforeRaf)
    + " · 节点对象复用=" + (sameObjects ? "5/5" : "否")
    + " · cx/cy 偏移=" + cxMoved + " 处");
  ok(env.state.reloads - beforeReloads === 0, "AC-e5 · location.reload 0 次");
  ok(env.stats.gRoot - beforeGRoot === 0,
    "AC-e5 · 布局函数（buildGraph ⇒ simulate）0 次：g-root 没有被重建");
  ok(env.stats.nodeG - beforeNodeG === 0, "AC-e5 · 没有新建任何 node-g 元素（图没重建）");
  ok(env.state.raf - beforeRaf === 0, "AC-e5 · 没有触发物理/动画帧");
  ok(sameObjects, "AC-e5 · 五个节点还是**原来那几个 DOM 对象**（同一性）");
  ok(cxMoved === 0, "AC-e5 · 所有节点的 cx/cy 一个都没动（布局没跳）", "动了 " + cxMoved + " 个");

  /* ── AC-e2：颜色真的变了 ─────────────────────────────────── */
  const tones = tonesOf(after);
  ok(tones.length >= 2, "AC-e2 · 切到中心度后节点色 ≥2 种（确实变了）", tones.join(" , "));
  ok(JSON.stringify(tones) !== JSON.stringify(tonesOf(before)),
    "AC-e2 · 与切换前的配色不同（不是「换了图例没换色」）");
  ok(after.get("a") && after.get("a").tone === TIER.hub.color,
    "AC-e2 · deg 最高的 a（deg=3=P95）⇒ 枢纽色", after.get("a") ? after.get("a").tone : "");
  ok(after.get("b") && after.get("b").tone === TIER.core.color,
    "AC-e2 · b（deg=1，P80≤1<P95）⇒ 中心色", after.get("b") ? after.get("b").tone : "");
  ok(after.get("e") && after.get("e").tone === TIER.leaf.color,
    "AC-e2 · e（deg=0<P50）⇒ 边缘色", after.get("e") ? after.get("e").tone : "");
  ok(after.get("a") && after.get("a").glow === "url(#glow-d0)",
    "AC-e2 · 枢纽用派生出来的泛光渐变 glow-d0（色值仍只有 DEGREE_TIERS 一份）",
    after.get("a") ? after.get("a").glow : "");

  /* ── AC-e4：图例跟着模式变 ───────────────────────────────── */
  const legendAfter = text(env, "legend");
  ok(legendAfter.indexOf("中心度着色") >= 0,
    "AC-e4 · 图例换成了「中心度着色」", legendAfter);
  ok(legendAfter !== legendBefore, "AC-e4 · 图例文本确实变了（不是同一段文字）");
  ok(legendAfter.indexOf("枢纽") >= 0 && legendAfter.indexOf("中心") >= 0
    && legendAfter.indexOf("一般") >= 0 && legendAfter.indexOf("边缘") >= 0,
    "AC-e4 · 四档文案都在图例里", legendAfter);
  ok(legendAfter.indexOf("大小与颜色同源") >= 0,
    "AC-e4 · 图例写明「大小与颜色同源」（大小本来就被 deg 控制）", legendAfter);
  for (const bad of ["需取数", "不可查", "API 源", "库源"]) {
    ok(legendAfter.indexOf(bad) < 0,
      "AC-e4 · 中心度图例里 ⛔ 不许出现「" + bad + "」（否则颜色在撒谎）", legendAfter);
  }

  /* ── AC-e1：选择进 sessionStorage，不进 URL ──────────────── */
  ok(env.session.get(COLOR_KEY) === "degree",
    "AC-e1 · 选择写进了 sessionStorage", String(env.session.get(COLOR_KEY)));
  ok(env.state.urlWrites === 0, "AC-e1 · 切换没有写 URL（search/hash 一次都没碰）");
  ok(env.requests.every((r) => r.url.indexOf("color") < 0), "AC-e1 · 没有把模式拼进任何请求");

  /* ── 互不污染：切回可查性必须**逐字还原** ────────────────── */
  sel.value = "verifiability";
  env.dispatch(sel, "change", {});
  await tick(10);
  const back = nodePaint(env);
  const backTones = [...back.entries()].map(([k, p]) => k + "=" + p.tone).sort();
  ok(JSON.stringify(backTones) === JSON.stringify(beforeTones),
    "切回可查性：颜色**逐字还原**（切来切去不会锁死）",
    "now " + backTones.join(",") + " / was " + beforeTones.join(","));
  ok(text(env, "legend") === legendBefore, "切回可查性：图例也逐字还原");
  ok(env.session.get(COLOR_KEY) === "verifiability", "切回后 sessionStorage 也回到 verifiability");
  ok(env.stats.gRoot - beforeGRoot === 0 && env.stats.nodeG - beforeNodeG === 0,
    "两次切换都没有重建图（g-root / node-g 新建数仍是 0）");
  return env;
}

// ══════════════════════════════════════════════════════════════
// S3：分位数四档的 exact 边界（AC-e3）
// ══════════════════════════════════════════════════════════════
/* Havel–Hakimi：把想要的度序列变成一张真图（⛔ 不手搓边，度序列一改就崩） */
function edgesFromDegrees(deg) {
  const rem = deg.slice();
  const edges = [];
  for (let guard = 0; guard < 10000; guard++) {
    let v = -1;
    for (let i = 0; i < rem.length; i++) if (rem[i] > 0 && (v < 0 || rem[i] > rem[v])) v = i;
    if (v < 0) break;
    const need = rem[v];
    rem[v] = 0;
    const order = rem.map((d, i) => i).filter((i) => i !== v && rem[i] > 0)
      .sort((a, b) => rem[b] - rem[a] || a - b);
    if (order.length < need) throw new Error("度序列不可图化：" + deg.join(","));
    for (let k = 0; k < need; k++) { rem[order[k]] -= 1; edges.push([v, order[k]]); }
  }
  const got = new Array(deg.length).fill(0);
  edges.forEach(([a, b]) => { got[a] += 1; got[b] += 1; });
  if (got.join(",") !== deg.join(",")) {
    throw new Error("构造出来的度序列对不上：" + got.join(",") + " ≠ " + deg.join(","));
  }
  return edges;
}

async function scenarioQuantileExact() {
  section("[S3] 分位数 exact：20 个类，deg 0..9 各两个 ⇒ P50=4 / P80=7 / P95=9，四档边界逐一点名");

  const DEG = [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9];
  const names = DEG.map((_, i) => "c" + i);
  const pairs = edgesFromDegrees(DEG).map(([a, b]) => [names[a], names[b]]);

  const classOf = (deg) => (deg >= 9 ? "hub" : deg >= 7 ? "core" : deg >= 4 ? "mid" : "leaf");

  const env = await bootEnv({
    token: TOKEN,
    colorMode: "degree",             /* 模拟「刷新后保持上次选择」 */
    handler(req) {
      const base = healthHandler("ontology-only")(req);
      if (base) return base;
      if (req.method === "GET" && req.url.indexOf("/api/catalog") === 0) {
        return { status: 200, body: catBody("probe-proj", names, pairs) };
      }
      return null;
    }
  });

  /* ① 刷新后保持：下拉自己对齐到 sessionStorage 的值 */
  const sel = byId(env, "color-mode");
  ok(sel.value === "degree", "AC-e1 · 刷新后下拉回到上次的选择（degree）", sel.value);

  const legend = text(env, "legend");
  ok(legend.indexOf("枢纽（≥9）") >= 0,
    "AC-e3 · P95 = 9（最近秩：ceil(0.95×20)=19 ⇒ 第 19 个 = 9）", legend);
  ok(legend.indexOf("中心（7–8）") >= 0,
    "AC-e3 · P80 = 7（ceil(0.80×20)=16 ⇒ 第 16 个 = 7）", legend);
  ok(legend.indexOf("一般（4–6）") >= 0,
    "AC-e3 · P50 = 4（ceil(0.50×20)=10 ⇒ 第 10 个 = 4）", legend);
  ok(legend.indexOf("边缘（<4）") >= 0, "AC-e3 · 边缘 = deg < 4", legend);

  /* ② 每个节点的颜色 = 它那一档（20 个全查，边界节点单独点名） */
  const paint = nodePaint(env);
  ok(paint.size === 20, "AC-e3 · 图上有 20 个节点", String(paint.size));
  let wrong = 0;
  const table = [];
  DEG.forEach((deg, i) => {
    const key = names[i];
    const want = TIER[classOf(deg)].color;
    const got = paint.get(key) ? paint.get(key).tone : "(缺)";
    if (got !== want) wrong += 1;
    table.push("c" + i + ":deg " + deg + "→" + classOf(deg));
  });
  console.log("  · 度数 → 档位：" + table.join("  "));
  ok(wrong === 0, "AC-e3 · 20 个节点的颜色与它自己的 deg 档位**逐一对上**", "错 " + wrong + " 个");

  /* ③ 边界值 exact：P50/P80/P95 上下一格各点名一次 */
  const boundary = [
    ["c18", 9, "hub"],   /* deg = P95 ⇒ 枢纽 */
    ["c16", 8, "core"],  /* deg = P95−1 ⇒ 中心（⛔ 不是枢纽） */
    ["c14", 7, "core"],  /* deg = P80 ⇒ 中心 */
    ["c12", 6, "mid"],   /* deg = P80−1 ⇒ 一般（⛔ 不是中心） */
    ["c8", 4, "mid"],    /* deg = P50 ⇒ 一般 */
    ["c6", 3, "leaf"]    /* deg = P50−1 ⇒ 边缘（⛔ 不是一般） */
  ];
  let bad = 0;
  boundary.forEach(([key, deg, tier]) => {
    const got = paint.get(key) ? paint.get(key).tone : null;
    if (got !== TIER[tier].color) {
      bad += 1;
      console.log("      ✗ 边界 " + key + "（deg=" + deg + "）应为 " + tier + "，实际 " + got);
    }
  });
  ok(bad === 0,
    "AC-e3 · 六个边界节点（P95 / P95−1 / P80 / P80−1 / P50 / P50−1）判档全部 exact");

  /* ④ 分位数不是绝对阈值：同样的代码在 5 个类上也必须分得出四档里至少三档 */
  const small = ["x0", "x1", "x2", "x3", "x4"];
  const smallEdges = [["x0", "x1"], ["x0", "x2"], ["x0", "x3"], ["x0", "x4"], ["x1", "x2"]];
  const envSmall = await bootEnv({
    token: TOKEN,
    colorMode: "degree",
    handler(req) {
      const base = healthHandler("ontology-only")(req);
      if (base) return base;
      if (req.method === "GET" && req.url.indexOf("/api/catalog") === 0) {
        return { status: 200, body: catBody("probe-proj", small, smallEdges) };
      }
      return null;
    }
  });
  const smallTones = tonesOf(nodePaint(envSmall));
  ok(smallTones.length >= 3,
    "AC-e3 · 5 个类的图上也能分出 ≥3 档（绝对阈值做不到这件事）", smallTones.join(" , "));
  /* 退化区间：5 个类时 P50 == P80 ⇒ 「一般」是空档，图例必须说「无」，
     ⛔ 不许印一个反的区间（"1–0"）或凭空一个数字 */
  const smallLegend = text(envSmall, "legend");
  ok(smallLegend.indexOf("一般（无）") >= 0,
    "AC-e3 · 空档在阈值位置上写「无」（不印反的区间）", smallLegend);
  return env;
}

// ══════════════════════════════════════════════════════════════
// S4：sessionStorage 的两种坏值（AC-e1）
// ══════════════════════════════════════════════════════════════
async function scenarioStorageEdgeCases() {
  section("[S4] sessionStorage：坏值退回默认；模式 A 的选择也记得住");

  const envBad = await bootEnv({
    token: TOKEN,
    colorMode: "nonsense",
    handler(req) {
      const base = healthHandler("ontology-only")(req);
      if (base) return base;
      if (req.method === "GET" && req.url.indexOf("/api/catalog") === 0) {
        return { status: 200, body: catBody("probe-proj", ["s1"], []) };
      }
      return null;
    }
  });
  ok(byId(envBad, "color-mode").value === "verifiability",
    "坏值（nonsense）⇒ 退回默认「可查性」", byId(envBad, "color-mode").value);
  ok(text(envBad, "legend").indexOf("API 源") >= 0,
    "坏值 ⇒ 图例也是可查性那套（不是空图例）");

  const envA = await bootEnv({
    token: TOKEN,
    colorMode: "verifiability",
    handler(req) {
      const base = healthHandler("query")(req);
      if (base) return base;
      if (req.method === "GET" && req.url.indexOf("/api/catalog") === 0) {
        return { status: 200, body: catBody("probe-proj", ["s2"], []) };
      }
      return null;
    }
  });
  ok(byId(envA, "color-mode").value === "verifiability" && text(envA, "legend").indexOf("API 源") >= 0,
    "存的是 verifiability ⇒ 刷新后还是可查性");
  ok(nodePaint(envA).get("s2").tone === TONE_OK,
    "存的是 verifiability ⇒ 节点用的是模式 A 的颜色（蓝 #6ec8ff）",
    nodePaint(envA).get("s2").tone);
  return envA;
}

// ══════════════════════════════════════════════════════════════
// 主流程
// ══════════════════════════════════════════════════════════════
(async function main() {
  console.log("REQ-159 P1c-2b · Studio 节点着色探针（假 DOM，跑 src/studio/studio.js 那一份）\n");
  await scenarioDefaultVerifiability();
  await scenarioSwitchToDegree();
  await scenarioQuantileExact();
  await scenarioStorageEdgeCases();

  console.log("\n──────────────────────────────────────────────");
  if (failures.length) {
    console.log("✗ 失败 " + failures.length + " 条（通过 " + pass + " 条）");
    failures.forEach((f) => console.log("  · " + f));
    process.exit(1);
  }
  console.log("✓ 全部通过：" + pass + " 条断言");
  process.exit(0);
})().catch((e) => {
  console.error("探针自身出错：", e);
  process.exit(1);
});
