#!/usr/bin/env node
// ⭐ REQ-159 **P1c-2** · Studio 交互层判据（Node + 假 DOM；本机没有 headless 浏览器）。
//
// 它把 `src/studio/studio.js` **原样**（不复制、不改写）放进一个最小 DOM 桩里跑，
// 静态节点由 `src/studio/index.html` 里的 id 生成 —— 于是验的是**线上那一份代码**，
// 不是另写一遍的模拟。
//
// | 判据 | 验的是什么 |
// | --- | --- |
// | AC-d2 | 项目下拉列出 2 个项目；切换后 `?project=` 变了、**SVG 里的 node-g 数量变了**（且⛔没有整页刷新） |
// | AC-d3 | `key_label` 空 ⇒ 前端拦住：`POST /api/projects` **一次都不发** |
// | AC-d4 | 上传的 `Content-Type` 是 `octet-stream`、body 是**原始字节**（逐字节相等），⛔ 不是 multipart |
// | AC-d5 | `requires_db=true` 且没测通 ⇒ 「开始解析」按钮 `disabled`；测通后才可点 |
// | AC-d6 | `already_imported:true` ⇒ 界面显示**成功**（「幂等成功」），没有错误框，可以继续下一步 |
// | AC-d7 | 激活前有二次确认，文案含「会影响对外服务」；确认之前**一条 activate 请求都不发** |
// | AC-d8 | 无 admin 口令（403 `key.not_admin`）⇒ 操作按钮 disabled + 提示「需要 admin key」，图照样画出来 |
// | AC-d9 | 404 ⇒ 提示「服务端未就绪」；403 ⇒「需要 admin key」；⛔ 不登出、⛔ 不白屏 |
// | 额外 | 查看 key 的固定一行「本次查看会记录一条审计」 |
//
// 跑法：node tests/fixtures/req159d/studio_probe.js
// 退出码：0 = 全过；1 = 有判据失败（可直接当判据用）。

"use strict";

const fs = require("fs");
const path = require("path");
const vm = require("vm");

const ROOT = path.resolve(__dirname, "..", "..", "..");
const SCRIPT = fs.readFileSync(path.join(ROOT, "src", "studio", "studio.js"), "utf8");
const HTML = fs.readFileSync(path.join(ROOT, "src", "studio", "index.html"), "utf8");

const TOKEN = "FAKE-REQ159D-ADMIN-0123456789abcdef";
const TOKEN_KEY = "ontok_studio_token";

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
  const innerHtml = { writes: 0 };
  const registry = new Map();

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
        if (k === "id") { node.id = String(v); registry.set(node.id, node); }
        if (k === "class") node.className = String(v);
        if (k === "disabled") node.disabled = true;
      },
      getAttribute(k) {
        if (k === "class") return node.className;
        return Object.prototype.hasOwnProperty.call(node._attrs, k) ? node._attrs[k] : null;
      },
      closest(sel) {
        let cur = node;
        while (cur) {
          if (sel === ".projsel" && String(cur.className || "").split(/\s+/).includes("projsel")) return cur;
          cur = cur.parentNode;
        }
        return null;
      },
      getBoundingClientRect() { return { left: 0, top: 0, width: 1200, height: 800, right: 1200, bottom: 800 }; },
      getBBox() { return { x: -400, y: -300, width: 800, height: 600 }; },
      select() {}, focus() {}, blur() {}, scrollIntoView() {},
      querySelectorAll() { return []; },
      click() {
        // 与浏览器一致：disabled 的按钮点不动（AC-d5 / AC-d8 就靠这条语义）
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
      set() {
        innerHtml.writes += 1;
        throw new Error("probe: innerHTML 被赋值（Studio 的纪律是 createElement + textContent）");
      }
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

  return { makeNode, document, registry, innerHtml, dispatch };
}

function makeEnv(opts) {
  opts = opts || {};
  const dom = createDom();
  const document = dom.document;
  const requests = [];
  const state = { handler: opts.handler, reloads: 0, confirmAnswer: true };

  function fetchStub(url, init) {
    init = init || {};
    const method = String(init.method || "GET").toUpperCase();
    const headers = init.headers || {};
    const req = {
      method,
      url: String(url),
      headers,
      body: init.body,
      contentType: headers["Content-Type"] || headers["content-type"] || "",
      tag: Object.prototype.toString.call(init.body)
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
    _innerHtml: dom.innerHtml,
    _state: state,
    _listeners: {}
  };
  sandbox.window = sandbox;
  sandbox.globalThis = sandbox;
  sandbox.window.innerWidth = 1280;
  sandbox.window.innerHeight = 800;
  sandbox.window.requestAnimationFrame = function () { return 0; };   // 定帧：图谱不跑动画
  sandbox.window.location = {
    reload() { state.reloads += 1; throw new Error("probe: 整页刷新（切项目不该刷新）"); }
  };
  sandbox.window.confirm = function () { return state.confirmAnswer !== false; };
  sandbox.window.addEventListener = function (t, fn) {
    (sandbox._listeners[t] = sandbox._listeners[t] || []).push(fn);
  };
  if (opts.token !== null) session.set(TOKEN_KEY, opts.token || TOKEN);

  vm.createContext(sandbox);
  return { sandbox, document, registry: dom.registry, requests, state, dispatch: dom.dispatch, session };
}

// ── 小工具 ─────────────────────────────────────────────────────
const tick = (ms) => new Promise((r) => setTimeout(r, ms || 25));

function byId(env, id) { return env.document.getElementById(id); }
function text(env, id) { const n = byId(env, id); return n ? n.textContent : ""; }
function bodyText(env) { return text(env, "wiz-body"); }
function isDisabled(env, id) { const n = byId(env, id); return !n ? null : !!n.disabled; }
function classesOf(node) { return String(node.className || "").split(/\s+/).filter(Boolean); }
function hasClass(node, c) { return !!node && classesOf(node).includes(c); }
function walk(node, fn) {
  if (!node) return;
  fn(node);
  (node.children || []).forEach((c) => walk(c, fn));
}
function countNodes(env) {
  let n = 0;
  walk(byId(env, "graph"), (el) => { if (hasClass(el, "node-g")) n += 1; });
  return n;
}
function menuItems(env) {
  const box = byId(env, "proj-menu");
  return (box ? box.children : []).filter((c) => hasClass(c, "proj-item"));
}
function alertsIn(env, id) {
  const out = [];
  walk(byId(env, id), (el) => {
    if (hasClass(el, "alert")) out.push({ cls: el.className, text: el.textContent });
  });
  return out;
}
function posts(env, pred) {
  return env.requests.filter((r) => r.method === "POST" && (!pred || pred(r)));
}
function lastReq(env, pred) {
  const list = env.requests.filter(pred);
  return list.length ? list[list.length - 1] : null;
}

// ── 目录（catalog）响应 ───────────────────────────────────────
function catBody(project, classes, relations) {
  const items = [];
  classes.forEach((c) => {
    items.push({
      kind: "class", name: c, display_name: c + " 类", iri: "https://ontok.local/" + project + "#" + c,
      property_count: 2, relation_count: (relations || []).length
    });
  });
  (relations || []).forEach((r) => {
    items.push({ kind: "relation", name: r[0] + "__" + r[1], from: r[0], to: r[1], evidence_level: "unverified" });
  });
  return {
    project: project, version: 3, page: 1, page_size: 200, has_more: false,
    counts: { classes: classes.length, properties: classes.length * 2, relations: (relations || []).length },
    items
  };
}

// ══════════════════════════════════════════════════════════════
// 场景 1：admin 全链路（AC-d2 / d3 / d4 / d5 正向 / d6 / d7 / 查看 key）
// ══════════════════════════════════════════════════════════════
async function scenarioAdminHappyPath() {
  section("[S1] admin：下拉切换 → 建项目（key_label 门）→ 上传 → DSN → 解析(幂等) → 激活 → 查看 key");

  const PROJECTS = [
    { name: "cloud-resource", label: "腾讯云资源本体", active_version: 5, frozen: true, entity_types: 2, link_types: 1 },
    { name: "cost-insight", label: null, active_version: 2, frozen: true, entity_types: 5, link_types: 0 }
  ];
  const afterCreate = PROJECTS.concat([{
    name: "my-ontology", label: "P1c-2 测试", active_version: null,
    frozen: false, entity_types: 0, link_types: 0
  }]);
  const BUNDLE_BYTES = [80, 75, 3, 4, 20, 0, 0, 0, 8, 0, 1, 2, 3, 4];

  let projectsServed = PROJECTS;
  const env = makeEnv({
    token: TOKEN,
    handler(req) {
      const u = req.url;
      if (req.method === "GET" && u === "/api/health") {
        return { status: 200, body: { project: "cloud-resource", ontology_version: 5, mode: "ontology-only" } };
      }
      if (req.method === "GET" && u === "/api/projects") {
        return { status: 200, body: { projects: projectsServed } };
      }
      if (req.method === "GET" && u.indexOf("/api/catalog") === 0) {
        if (u.indexOf("project=cloud-resource") >= 0) {
          return { status: 200, body: catBody("cloud-resource", ["cvm_instance", "cbs_disk"], [["cvm_instance", "cbs_disk"]]) };
        }
        if (u.indexOf("project=cost-insight") >= 0) {
          return { status: 200, body: catBody("cost-insight", ["a1", "a2", "a3", "a4", "a5"], []) };
        }
        if (u.indexOf("project=my-ontology") >= 0) {
          return { status: 200, body: catBody("my-ontology", ["n1"], []) };
        }
        return { status: 200, body: catBody("cloud-resource", ["cvm_instance", "cbs_disk"], []) };
      }
      if (req.method === "POST" && u === "/api/projects") {
        projectsServed = afterCreate;
        return {
          status: 201,
          body: {
            project: "my-ontology", label: "P1c-2 测试", key: "ontok_q_PROBEKEY0001",
            key_kind: "query", key_label: "数字员工 · 探针", key_digest: "sha256:aaaa",
            bound_project: "my-ontology", created_at: "2026-10-27T06:00:00.000Z"
          }
        };
      }
      if (req.method === "POST" && u === "/api/projects/my-ontology/bundles") {
        return {
          status: 201,
          body: {
            bundle_id: "bdl_probe", project: "my-ontology", state: "uploaded", requires_db: true,
            components: {}, counts: { entity_types: 1, link_types: 0, properties: 2 },
            sha256_verified: true, bytes: BUNDLE_BYTES.length,
            bundle_sha256: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            uploaded_at: "2026-10-27T06:01:00.000Z"
          }
        };
      }
      if (req.method === "PUT" && u === "/api/projects/my-ontology/database") {
        return { status: 200, body: { database_id: "db_x", project: "my-ontology", dialect: "postgresql", host: "h", database: "d", user: "u", updated_at: "x", last_check_ok: null } };
      }
      if (req.method === "POST" && u === "/api/projects/my-ontology/database/test") {
        return { status: 200, body: { ok: true, latency_ms: 12, server_version: "PostgreSQL 15.4", error: null } };
      }
      if (req.method === "POST" && u === "/api/projects/my-ontology/bundles/bdl_probe/import") {
        return {
          status: 200,
          body: {
            bundle_id: "bdl_probe", project: "my-ontology", version: 6,
            version_id: "00000000-0000-0000-0000-000000000001", state: "imported", frozen: false,
            counts: { entity_types: 1, link_types: 0, properties: 2, action_types: 0, functions: 0, concept_groups: 0 },
            warnings: [], already_imported: true
          }
        };
      }
      if (req.method === "POST" && u === "/api/projects/my-ontology/bundles/bdl_probe/activate") {
        return {
          status: 200,
          body: {
            bundle_id: "bdl_probe", project: "my-ontology", version: 6,
            version_id: "00000000-0000-0000-0000-000000000001", frozen: true,
            frozen_at: "2026-10-27T06:02:00.000Z",
            active_version_id: "00000000-0000-0000-0000-000000000001", state: "activated"
          }
        };
      }
      if (req.method === "GET" && u === "/api/projects/my-ontology/key") {
        return {
          status: 200,
          body: { project: "my-ontology", key: "ontok_q_PROBEKEY0001", label: "数字员工 · 探针", created_at: "2026-10-27T06:00:00.000Z", revoked_at: null }
        };
      }
      return null;
    }
  });

  vm.runInContext(SCRIPT, env.sandbox, { filename: "studio.js" });
  await tick(40);

  // ── 启动后的顶栏 ──
  ok(!isDisabled(env, "proj-add"), "admin 判定通过 ⇒ ＋ 可用");
  ok(!isDisabled(env, "bundle-open"), "admin 判定通过 ⇒ 「上传构建包」可用");
  ok(!isDisabled(env, "key-view"), "admin 判定通过 ⇒ 「查看 key」可用");
  ok(text(env, "crumb-project") === "cloud-resource", "顶栏显示当前项目（health.project 命中的那个）", text(env, "crumb-project"));
  ok(text(env, "ops-hint") === "", "admin 时顶栏没有「需要 admin key」提示", text(env, "ops-hint"));
  const firstCat = lastReq(env, (r) => r.url.indexOf("/api/catalog") === 0);
  ok(!!firstCat && firstCat.url.indexOf("project=cloud-resource") >= 0,
    "admin 的 catalog 请求带 ?project=<当前项目>（多项目模式下不带会 403）", firstCat && firstCat.url);

  // ── AC-d2（前半）：下拉里 ≥2 个项目 ──
  byId(env, "proj-pick").click();
  await tick(5);
  const items = menuItems(env);
  ok(items.length === 2, "AC-d2 · 项目下拉列出 2 个项目", "实际 " + items.length);
  ok(items.length >= 2, "AC-d2 · 至少 2 个（判据要求 ≥2）");
  ok(hasClass(byId(env, "proj-menu"), "is-open"), "点项目名 ⇒ 菜单展开（不是整页刷新）");
  const nodeCountBefore = countNodes(env);
  ok(nodeCountBefore === 2, "AC-d2 · 切前 SVG 里的节点数 = cloud-resource 的 2 个类", "实际 " + nodeCountBefore);

  // ── AC-d2（后半）：切到 cost-insight ⇒ 节点数变、URL 带新项目、不刷新 ──
  const reloadsBefore = env.state.reloads;
  items[1].click();
  await tick(40);
  const nodeCountAfter = countNodes(env);
  ok(nodeCountAfter === 5, "AC-d2 · 切后 SVG 里的节点数 = cost-insight 的 5 个类", "实际 " + nodeCountAfter);
  ok(nodeCountAfter !== nodeCountBefore, "AC-d2 · 切换项目后**图的节点数确实变了**",
    nodeCountBefore + " → " + nodeCountAfter);
  ok(text(env, "crumb-project") === "cost-insight", "顶栏跟着换了项目名", text(env, "crumb-project"));
  const switchedCat = lastReq(env, (r) => r.url.indexOf("/api/catalog") === 0);
  ok(!!switchedCat && switchedCat.url.indexOf("project=cost-insight") >= 0,
    "AC-d2 · 切换后重新拉 catalog，且带 ?project=cost-insight", switchedCat && switchedCat.url);
  ok(env.state.reloads === reloadsBefore, "AC-d2 · 切项目**没有整页刷新**（location.reload 一次都没调）");
  ok(hasClass(byId(env, "ops-banner"), "is-on") && text(env, "ops-banner").indexOf("已切到项目") >= 0,
    "切换成功有可见回执", text(env, "ops-banner"));

  // ── AC-d3：＋ 建项目，key_label 空 ⇒ 一次请求都不发 ──
  byId(env, "proj-add").click();
  await tick(5);
  ok(hasClass(byId(env, "wiz-modal"), "is-open"), "点 ＋ ⇒ 向导弹窗打开");
  ok(text(env, "wiz-title") === "新建项目", "向导第 1 步是「新建项目」", text(env, "wiz-title"));
  byId(env, "wiz-name").value = "my-ontology";
  byId(env, "wiz-keylabel").value = "";        // ⛔ 空
  const postsBefore = posts(env).length;
  byId(env, "wiz-next").click();
  await tick(20);
  ok(posts(env).length === postsBefore,
    "AC-d3 · key_label 空 ⇒ **一条 POST 都不发**（前端拦住）",
    "多发 " + (posts(env).length - postsBefore) + " 条");
  ok(bodyText(env).indexOf("必填") >= 0, "AC-d3 · 界面上说清了「必填」", bodyText(env));
  ok(alertsIn(env, "wiz-body").some((a) => String(a.cls).indexOf("alert-err") >= 0),
    "AC-d3 · 拦住时显示的是错误框（不是悄悄不动）");

  // ── 填好再建 ⇒ 一次性 key 出现 ──
  byId(env, "wiz-keylabel").value = "数字员工 · 探针";
  byId(env, "wiz-next").click();
  await tick(40);
  const created = lastReq(env, (r) => r.method === "POST" && r.url === "/api/projects");
  ok(!!created, "填好后发出 POST /api/projects");
  const createdBody = created ? JSON.parse(String(created.body)) : {};
  ok(createdBody.project === "my-ontology" && createdBody.key_label === "数字员工 · 探针",
    "建项目的 body 带 project + key_label", JSON.stringify(created && created.body));
  ok(created && String(created.contentType).indexOf("application/json") >= 0,
    "建项目用 JSON（不是表单）", created && created.contentType);
  ok(text(env, "wiz-body").indexOf("ontok_q_PROBEKEY0001") >= 0,
    "一次性 key 显示在向导里", text(env, "wiz-body"));
  ok(text(env, "wiz-body").indexOf("只显示这一次") >= 0, "并说明「只显示这一次」");
  ok(text(env, "wiz-title") === "上传构建包", "建完直接进上传向导（不用自己再找入口）", text(env, "wiz-title"));

  // ── AC-d4：上传（octet-stream + 原始字节） ──
  const fileInput = byId(env, "wiz-file");
  fileInput.files = [{
    name: "demo.ontokbundle",
    arrayBuffer: () => Promise.resolve(new Uint8Array(BUNDLE_BYTES).buffer)
  }];
  byId(env, "wiz-upload").click();
  await tick(60);
  const up = lastReq(env, (r) => r.method === "POST" && r.url === "/api/projects/my-ontology/bundles");
  ok(!!up, "上传发出 POST /api/projects/:p/bundles");
  ok(up && up.contentType === "application/octet-stream",
    "AC-d4 · Content-Type 是 application/octet-stream", up && up.contentType);
  ok(up && up.tag === "[object ArrayBuffer]", "AC-d4 · body 是 ArrayBuffer（原始字节，不是字符串）", up && up.tag);
  if (up && up.tag === "[object ArrayBuffer]") {
    const got = Array.from(new Uint8Array(up.body));
    const same = got.length === BUNDLE_BYTES.length && got.every((b, i) => b === BUNDLE_BYTES[i]);
    ok(same, "AC-d4 · body 与文件**逐字节相等**（" + BUNDLE_BYTES.length + " 字节）", "got=" + got.join(","));
  } else {
    ok(false, "AC-d4 · body 逐字节比对（拿不到 ArrayBuffer ⇒ 视为红）");
  }
  ok(bodyText(env).indexOf("上传完成") >= 0, "上传成功有回执", bodyText(env));
  ok(!isDisabled(env, "wiz-next"), "上传成功后「下一步」可用");
  ok(text(env, "wiz-body").indexOf("requires_db = true") >= 0,
    "界面如实说这个包要 DSN", bodyText(env));

  // ── AC-d5（正向）：DSN 测通后「开始解析」才可点 ──
  byId(env, "wiz-next").click();
  await tick(5);
  ok(text(env, "wiz-title") === "配置源数据库", "requires_db ⇒ 第 3 步是「配置源数据库」", text(env, "wiz-title"));
  byId(env, "wiz-dsn").value = "postgresql://u:p@127.0.0.1:5432/db";
  byId(env, "wiz-dsn-save").click();
  await tick(60);
  const putDsn = lastReq(env, (r) => r.method === "PUT" && r.url === "/api/projects/my-ontology/database");
  const testDsn = lastReq(env, (r) => r.method === "POST" && r.url === "/api/projects/my-ontology/database/test");
  ok(!!putDsn && !!testDsn, "先 PUT 存 DSN，再 POST …/database/test 测连通（测的是已存的那一条）");
  ok(bodyText(env).indexOf("连通性通过") >= 0, "测通后有明确回执", bodyText(env));
  byId(env, "wiz-next").click();
  await tick(5);
  ok(isDisabled(env, "wiz-import") === false, "AC-d5 · 测通后「开始解析」可点");

  // ── AC-d6：already_imported:true 当**成功** ──
  byId(env, "wiz-import").click();
  await tick(40);
  const imp = lastReq(env, (r) => r.method === "POST" && r.url.indexOf("/import") >= 0);
  ok(!!imp, "解析发出 POST …/import");
  const impAlerts = alertsIn(env, "wiz-body");
  ok(impAlerts.some((a) => String(a.cls).indexOf("alert-ok") >= 0), "AC-d6 · 显示成功框");
  ok(impAlerts.every((a) => String(a.cls).indexOf("alert-err") < 0), "AC-d6 · **没有**错误框");
  ok(bodyText(env).indexOf("幂等成功") >= 0, "AC-d6 · 重复解析（already_imported:true）按幂等成功渲染", bodyText(env));
  ok(bodyText(env).indexOf("v6") >= 0, "AC-d6 · 界面给出沿用的版本号", bodyText(env));
  ok(!isDisabled(env, "wiz-next"), "AC-d6 · 解析（幂等）后仍可继续下一步");
  ok(bodyText(env).indexOf("关系类型") >= 0, "解析结果展示 counts", bodyText(env));

  // ── AC-d7：激活的二次确认 ──
  byId(env, "wiz-next").click();
  await tick(5);
  ok(text(env, "wiz-title") === "冻结并激活", "第 5 步是激活", text(env, "wiz-title"));
  ok(byId(env, "wiz-confirm") === null, "AC-d7 · 刚进来没有确认框（要用户先点一次）");
  const actBefore = posts(env, (r) => r.url.indexOf("/activate") >= 0).length;
  byId(env, "wiz-ask-activate").click();
  await tick(5);
  const confirm = byId(env, "wiz-confirm");
  ok(!!confirm, "AC-d7 · 点「冻结并激活」弹出二次确认");
  ok(!!confirm && confirm.textContent.indexOf("会影响对外服务") >= 0,
    "AC-d7 · 确认文案含「会影响对外服务」", confirm && confirm.textContent);
  ok(posts(env, (r) => r.url.indexOf("/activate") >= 0).length === actBefore,
    "AC-d7 · 确认之前**一条 activate 请求都不发**");
  byId(env, "wiz-do-activate").click();
  await tick(80);
  ok(posts(env, (r) => r.url.indexOf("/activate") >= 0).length === actBefore + 1,
    "AC-d7 · 确认后只发 1 条 activate");
  ok(bodyText(env).indexOf("已激活") >= 0, "AC-d7 · 激活成功有回执", bodyText(env));
  ok(text(env, "ops-banner").indexOf("已激活") >= 0, "激活后图上回执也更新", text(env, "ops-banner"));

  // ── 查看 key：固定一行审计提示 ──
  byId(env, "wiz-next").click();     // 完成 → 关闭
  await tick(10);
  ok(!hasClass(byId(env, "wiz-modal"), "is-open"), "「完成」关闭向导");
  byId(env, "key-view").click();
  await tick(40);
  const keyReq = lastReq(env, (r) => r.method === "GET" && r.url.indexOf("/key") >= 0);
  ok(!!keyReq, "查看 key 发 GET …/key（不是缓存里的旧值）", keyReq && keyReq.url);
  ok(text(env, "key-body").indexOf("ontok_q_PROBEKEY0001") >= 0, "key 明文显示在弹窗里");
  ok(text(env, "key-body").indexOf("本次查看会记录一条审计") >= 0,
    "固定一行「本次查看会记录一条审计」在", text(env, "key-body"));

  // ── 全局纪律：一次 innerHTML 都没有 ──
  ok(env.sandbox._innerHtml.writes === 0, "全程没有 innerHTML 赋值");

  return env;
}

// ══════════════════════════════════════════════════════════════
// 场景 2：AC-d5 的反向（requires_db && 没测通 ⇒ 解析 disabled）
// ══════════════════════════════════════════════════════════════
async function scenarioRequiresDbGate() {
  section("[S2] AC-d5 反向：requires_db 但没测通 ⇒ 「开始解析」disabled（测不通也留在 disabled）");

  let testOk = false;
  const env = makeEnv({
    token: TOKEN,
    handler(req) {
      const u = req.url;
      if (req.method === "GET" && u === "/api/health") return { status: 200, body: { project: "p1", ontology_version: 1 } };
      if (req.method === "GET" && u === "/api/projects") {
        return { status: 200, body: { projects: [{ name: "p1", label: null, active_version: 1, frozen: true, entity_types: 1, link_types: 0 }] } };
      }
      if (req.method === "GET" && u.indexOf("/api/catalog") === 0) return { status: 200, body: catBody("p1", ["c1"], []) };
      if (req.method === "POST" && u === "/api/projects/p1/bundles") {
        return { status: 201, body: { bundle_id: "bdl_gate", project: "p1", state: "uploaded", requires_db: true, components: {}, counts: { entity_types: 1, link_types: 0 }, bytes: 3, bundle_sha256: "ff", uploaded_at: "x" } };
      }
      if (req.method === "PUT" && u === "/api/projects/p1/database") {
        return { status: 200, body: { database_id: "db", project: "p1", dialect: "postgresql", host: "h", database: "d", user: "u", updated_at: "x", last_check_ok: null } };
      }
      if (req.method === "POST" && u === "/api/projects/p1/database/test") {
        return testOk
          ? { status: 200, body: { ok: true, latency_ms: 3, server_version: "PostgreSQL 15.4", error: null } }
          : { status: 200, body: { ok: false, latency_ms: 0, server_version: null, error: { code: "db.connect.auth", message: "口令错误（SQLSTATE 28P01）" } } };
      }
      if (req.method === "POST" && u.indexOf("/import") >= 0) {
        return { status: 200, body: { bundle_id: "bdl_gate", project: "p1", version: 2, frozen: false, counts: { entity_types: 1, link_types: 0 }, warnings: [], already_imported: false } };
      }
      return null;
    }
  });

  vm.runInContext(SCRIPT, env.sandbox, { filename: "studio.js" });
  await tick(40);

  byId(env, "bundle-open").click();     // 顶栏「上传构建包」⇒ 直接从上传那一步开始
  await tick(5);
  ok(text(env, "wiz-title") === "上传构建包", "顶栏「上传构建包」直接进上传步（不用先建项目）", text(env, "wiz-title"));
  byId(env, "wiz-file").files = [{ name: "g.ontokbundle", arrayBuffer: () => Promise.resolve(new Uint8Array([1, 2, 3]).buffer) }];
  byId(env, "wiz-upload").click();
  await tick(60);
  byId(env, "wiz-next").click();        // → 第 3 步（requires_db）
  await tick(5);
  ok(text(env, "wiz-title") === "配置源数据库", "requires_db ⇒ 走第 3 步");

  // ① 一次都没测 ⇒ 直接去第 4 步
  byId(env, "wiz-next").click();
  await tick(5);
  ok(text(env, "wiz-title") === "解析构建包", "不测也能去第 4 步看（门在「解析」按钮上）", text(env, "wiz-title"));
  ok(isDisabled(env, "wiz-import") === true,
    "AC-d5 · requires_db=true 且**没测通** ⇒ 「开始解析」disabled");
  ok(bodyText(env).indexOf("解析按钮是灰的") >= 0, "AC-d5 · 并说明为什么灰", bodyText(env));
  const impBefore = posts(env, (r) => r.url.indexOf("/import") >= 0).length;
  byId(env, "wiz-import").click();      // disabled ⇒ 点了也不发
  await tick(20);
  ok(posts(env, (r) => r.url.indexOf("/import") >= 0).length === impBefore,
    "AC-d5 · 点一个 disabled 的「开始解析」⇒ 一条 import 请求都不发");

  // ② 测了但**不通** ⇒ 仍然 disabled
  const prev = (byId(env, "wiz-foot").children || []).find((b) => b.textContent === "上一步");
  ok(!!prev, "第 4 步有「上一步」");
  if (prev) prev.click();
  await tick(5);
  ok(text(env, "wiz-title") === "配置源数据库", "回到第 3 步");
  byId(env, "wiz-dsn").value = "postgresql://u:bad@127.0.0.1:5432/db";
  testOk = false;
  byId(env, "wiz-dsn-save").click();
  await tick(60);
  ok(bodyText(env).indexOf("连通性没通过") >= 0, "测不通有明确回执（失败也是 200）", bodyText(env));
  byId(env, "wiz-next").click();
  await tick(5);
  ok(isDisabled(env, "wiz-import") === true, "AC-d5 · 测过但**没通过** ⇒ 仍然 disabled");

  // ③ 测通 ⇒ 可点
  const prev2 = (byId(env, "wiz-foot").children || []).find((b) => b.textContent === "上一步");
  if (prev2) prev2.click();
  await tick(5);
  testOk = true;
  byId(env, "wiz-dsn-save").click();
  await tick(60);
  byId(env, "wiz-next").click();
  await tick(5);
  ok(isDisabled(env, "wiz-import") === false, "AC-d5 · 测通之后「开始解析」可点");
  return env;
}

// ══════════════════════════════════════════════════════════════
// 场景 3：非 admin（403 key.not_admin）—— AC-d8 / AC-d9
// ══════════════════════════════════════════════════════════════
async function scenarioNotAdmin() {
  section("[S3] 403 key.not_admin：图照看、写操作 disabled + 「需要 admin key」、⛔ 不登出/不白屏");

  const env = makeEnv({
    token: "FAKE-QUERY-KEY-0123456789abcdef",
    handler(req) {
      const u = req.url;
      if (req.method === "GET" && u === "/api/health") return { status: 200, body: { project: "agent-proj", ontology_version: 2 } };
      if (req.method === "GET" && u === "/api/projects") {
        return { status: 403, body: { code: "key.not_admin", message: "写端点只接受 admin", hint: "让运维加一条 kind=admin" } };
      }
      if (req.method === "GET" && u.indexOf("/api/catalog") === 0) {
        return { status: 200, body: catBody("agent-proj", ["q1", "q2", "q3"], []) };
      }
      return null;
    }
  });

  vm.runInContext(SCRIPT, env.sandbox, { filename: "studio.js" });
  await tick(40);

  ok(countNodes(env) === 3, "AC-d9 · 403 时**图照样画出来**（不白屏）", "节点数 " + countNodes(env));
  ok(env.session.get(TOKEN_KEY) === "FAKE-QUERY-KEY-0123456789abcdef",
    "AC-d8 · 403 **不登出**（sessionStorage 里的口令还在）");
  ok(env.state.reloads === 0, "AC-d8 · 403 不刷新整页");
  ok(isDisabled(env, "proj-add") === true, "AC-d8 · 无 admin 口令 ⇒ ＋ disabled");
  ok(isDisabled(env, "bundle-open") === true, "AC-d8 · 无 admin 口令 ⇒ 「上传构建包」disabled");
  ok(isDisabled(env, "key-view") === true, "AC-d8 · 无 admin 口令 ⇒ 「查看 key」disabled");
  ok(isDisabled(env, "proj-pick") === true, "AC-d8 · 无 admin 口令 ⇒ 项目下拉 disabled");
  ok(text(env, "ops-hint").indexOf("需要 admin key") >= 0,
    "AC-d8 · 顶栏提示「需要 admin key」", text(env, "ops-hint"));
  ok(text(env, "ops-banner").indexOf("需要 admin key") >= 0,
    "AC-d8 · 图上也有可见提示（不是只有灰按钮）", text(env, "ops-banner"));
  ok(posts(env).length === 0, "AC-d8 · 一个写请求都没发（按钮 disabled 就真的点不动）");
  byId(env, "proj-add").click();
  await tick(5);
  ok(!hasClass(byId(env, "wiz-modal"), "is-open"), "AC-d8 · 点 disabled 的 ＋ 不会打开向导");
  return env;
}

// ══════════════════════════════════════════════════════════════
// 场景 4：服务端未就绪（404）—— AC-d9
// ══════════════════════════════════════════════════════════════
async function scenarioNotReady() {
  section("[S4] 404：显示「服务端未就绪」、按钮 disabled、图照画、⛔ 不白屏");

  const env = makeEnv({
    token: TOKEN,
    handler(req) {
      const u = req.url;
      if (req.method === "GET" && u === "/api/health") return { status: 200, body: { project: "old-proj", ontology_version: 1 } };
      if (req.method === "GET" && u === "/api/projects") {
        return { status: 404, body: { code: "NOT_FOUND", message: "没有这个路径。" } };
      }
      if (req.method === "GET" && u.indexOf("/api/catalog") === 0) {
        return { status: 200, body: catBody("old-proj", ["o1", "o2"], []) };
      }
      return null;
    }
  });

  vm.runInContext(SCRIPT, env.sandbox, { filename: "studio.js" });
  await tick(40);

  ok(countNodes(env) === 2, "AC-d9 · 404 时图照样画（不白屏）", "节点数 " + countNodes(env));
  ok(text(env, "ops-hint").indexOf("服务端未就绪") >= 0,
    "AC-d9 · 顶栏提示「服务端未就绪」", text(env, "ops-hint"));
  ok(isDisabled(env, "proj-add") === true && isDisabled(env, "bundle-open") === true && isDisabled(env, "key-view") === true,
    "AC-d9 · 404 时三个写入口都 disabled");
  ok(text(env, "crumb-project") === "old-proj", "404 时退回 health 说的那个项目名（与改造前同一句）", text(env, "crumb-project"));
  return env;
}

// ══════════════════════════════════════════════════════════════
// 场景 5：无口令（未登录）—— AC-d8
// ══════════════════════════════════════════════════════════════
async function scenarioNoToken() {
  section("[S5] 没口令：入口 disabled + 「需要 admin key」，不发任何写请求");

  const env = makeEnv({ token: null, handler() { return null; } });
  vm.runInContext(SCRIPT, env.sandbox, { filename: "studio.js" });
  await tick(30);

  ok(isDisabled(env, "proj-add") === true, "无口令 ⇒ ＋ disabled");
  ok(text(env, "ops-hint").indexOf("需要 admin key") >= 0, "无口令 ⇒ 提示「需要 admin key」", text(env, "ops-hint"));
  ok(text(env, "boot-title") === "需要口令", "不白屏：给的是「需要口令」那一步", text(env, "boot-title"));
  ok(env.requests.length === 0, "无口令 ⇒ 一条请求都不发（与改造前一致）", "实际 " + env.requests.length);
  return env;
}

// ══════════════════════════════════════════════════════════════
// 主流程
// ══════════════════════════════════════════════════════════════
(async function main() {
  console.log("REQ-159 P1c-2 · Studio 交互探针（假 DOM，跑 src/studio/studio.js 那一份）\n");
  await scenarioAdminHappyPath();
  await scenarioRequiresDbGate();
  await scenarioNotAdmin();
  await scenarioNotReady();
  await scenarioNoToken();

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
