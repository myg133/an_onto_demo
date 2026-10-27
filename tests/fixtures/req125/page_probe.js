#!/usr/bin/env node
// REQ-125 · 工作台页面壳的**交互层判据**（Node + 假 DOM；本机没有 headless 浏览器）。
//
// 它把 `src/console/page.rs` 里的 `<script>` **原样**抽出来，在一个最小 DOM 桩上跑，
// 于是下面每条判据验的是**线上那一份代码**，不是另写一遍的模拟：
//
// | 判据 | 落点 |
// | --- | --- |
// | AC-006 | 取消确认框 ⇒ **一次 fetch 都不发**；确认后才发（apply / 声明保存） |
// | AC-007 | 保存失败 ⇒ 编辑器内容**未被清空 / 回滚**，且显示 `code` / `message` / `hint` |
// | AC-009 | 网络失败显示「无法连接服务」+ 可读原因，不是「加载失败」 |
// | AC-010 | 把 `<img src=x onerror=alert(1)>` 交给渲染器 ⇒ 只进 `textContent`；假 DOM 的 `innerHTML` 赋值即抛 |
// | AC-003 | `remember()` 只写 sessionStorage；沙箱里**没有** localStorage（碰了就 ReferenceError） |
// | B-1 回归 | 管理端点的 401 / 403 **仍然**清口令 + 回登录态（安全语义没被改坏） |
// | REQ-161b | 构建包的门禁口径还在（`progress.pending` / `already_imported` / `reveal_key` / `dsnFlag` / 5 处 `guarded`），且**查询服务的残留 = 0** |
//
// ⚠️ REQ-161b（2026-10-27）：查询台与目录浏览两个视图已移除（它们打的是 `query serve`，
// 不是本地操作），所以本探针里原先的 **AC-008（四段元信息）** 与 **B-1（查询端点
// 404/403 ⇒「查询服务未接入」）** 两段一并删掉了 —— 被测的渲染点与错误分支已经不存在，
// 留着只会「一跑就炸」，让下一个人以为页面坏了。
//
// 跑法：
//   node tests/fixtures/req125/page_probe.js
//
// 退出码：0 = 全过；1 = 有判据失败（可直接当判据用）。

"use strict";

const fs = require("fs");
const path = require("path");
const vm = require("vm");

const ROOT = path.resolve(__dirname, "..", "..", "..");

// ── 取页面常量 ────────────────────────────────────────────────
const rust = fs.readFileSync(path.join(ROOT, "src", "console", "page.rs"), "utf8");
const m = rust.match(/r####"([\s\S]*?)"####;/);
if (!m) {
  console.error("找不到 CONSOLE_HTML 的原始字符串字面量");
  process.exit(1);
}
const HTML = m[1];
const sm = HTML.match(/<script>([\s\S]*?)<\/script>/);
if (!sm) {
  console.error("页面里没有 <script> 块");
  process.exit(1);
}
const SCRIPT = sm[1];

// ── 最小 DOM 桩 ──────────────────────────────────────────────
let innerHtmlWrites = 0;
function makeNode(tag) {
  const node = {
    nodeType: 1,
    tagName: String(tag || "div").toUpperCase(),
    id: "",
    className: "",
    _text: "",
    value: "",
    checked: false,
    hidden: false,
    disabled: false,
    title: "",
    href: "",
    download: "",
    style: {},
    children: [],
    parentNode: null,
    _listeners: {},
    appendChild(child) {
      this.children.push(child);
      child.parentNode = this;
      return child;
    },
    removeChild(child) {
      this.children = this.children.filter((c) => c !== child);
      return child;
    },
    addEventListener(type, fn) {
      (this._listeners[type] = this._listeners[type] || []).push(fn);
    },
    click() {},
    focus() {},
    setAttribute(k, v) { this[k] = v; }
  };
  Object.defineProperty(node, "textContent", {
    get() {
      // 与真 DOM 一致：自己的文本 + 子节点的文本。
      return this._text + this.children.map((c) => c.textContent || "").join("");
    },
    set(v) {
      this._text = v === undefined || v === null ? "" : String(v);
      this.children = [];
    }
  });
  Object.defineProperty(node, "innerHTML", {
    get() { return this._text; },
    set() {
      innerHtmlWrites += 1;
      throw new Error("probe: innerHTML 被赋值（AC-010：用户数据只能走 textContent）");
    }
  });
  Object.defineProperty(node, "firstChild", {
    get() { return this.children.length ? this.children[0] : null; }
  });
  return node;
}

const nodes = new Map();
function nodeOf(id) {
  if (!nodes.has(id)) {
    const n = makeNode("div");
    n.id = id;
    nodes.set(id, n);
  }
  return nodes.get(id);
}

const FETCHES = [];
let nextReply = null;
const sandbox = {
  console,
  setTimeout,
  clearTimeout,
  document: {
    getElementById: (id) => nodeOf(id),
    createElement: (tag) => makeNode(tag),
    body: makeNode("body")
  },
  sessionStorage: (() => {
    const map = new Map();
    return {
      getItem: (k) => (map.has(k) ? map.get(k) : null),
      setItem: (k, v) => map.set(k, String(v)),
      removeItem: (k) => map.delete(k),
      _map: map
    };
  })(),
  fetch: (url, init) => {
    FETCHES.push({ url: String(url), init: init || {} });
    // 有预置回复就先回它（测 404 / 403 / 401 分支），否则当作「网络不可达」。
    if (nextReply) {
      const r = nextReply;
      nextReply = null;
      return Promise.resolve(r);
    }
    return Promise.reject(new TypeError("probe: network disabled"));
  }
};
// 预置一次 HTTP 回复（模拟真实服务端返回的 JSON 错误体）。
sandbox.__reply = (status, body) => {
  nextReply = {
    ok: false,
    status,
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(body)
  };
};
sandbox.window = sandbox;
sandbox.globalThis = sandbox;

vm.createContext(sandbox);
vm.runInContext(SCRIPT, sandbox, { filename: "console-page-inline.js" });

const W = sandbox.window.WBCORE;
if (!W) {
  console.error("页面没有导出 WBCORE（测试钩子丢了？）");
  process.exit(1);
}

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
}
function req(cond, what, detail) {
  ok(cond, what, detail);
}

(async function main() {
  console.log("REQ-125 · 工作台页面探针（假 DOM，跑页面里那一份 JS）\n");

  // ── AC-006 · 取消确认 ⇒ 不发请求 ───────────────────────────
  console.log("[AC-006] 危险操作二次确认：取消不发请求");
  W.state.projects = { project: "probe-project", versions: [] };
  W.state.at = "PROBE-TOKEN";
  nodeOf("p-alias").value = "p1";
  nodeOf("msg").className = "";

  const before = FETCHES.length;
  W.setConfirm(() => Promise.resolve(false)); // 用户点「取消」
  W.actions.apply();
  await new Promise((r) => setTimeout(r, 10));
  req(FETCHES.length === before, "apply 取消后 fetch 次数为 0", "实际 " + (FETCHES.length - before) + " 次");
  req(nodeOf("p-apply-out").children.length === 0, "取消时连错误框都不该出现");

  W.setConfirm(() => Promise.resolve(true)); // 用户点「确认」
  const before2 = FETCHES.length;
  W.actions.apply();
  await new Promise((r) => setTimeout(r, 10));
  req(FETCHES.length === before2 + 1, "apply 确认后只发 1 次请求", "实际 " + (FETCHES.length - before2) + " 次");
  req(
    FETCHES[FETCHES.length - 1] && FETCHES[FETCHES.length - 1].url === "api/apply",
    "确认后打到 api/apply",
    FETCHES[FETCHES.length - 1] && FETCHES[FETCHES.length - 1].url
  );
  req(
    FETCHES[FETCHES.length - 1].init.headers.Authorization === "Bearer PROBE-TOKEN",
    "请求带 Authorization（口令只在内存 / sessionStorage）"
  );

  // 声明保存：取消同样不发请求（AC-006 的第三处）。
  nodeOf("c-text").value = '{"version":"9.9.9"}';
  nodeOf("c-kind").value = "exclusions";
  W.setConfirm(() => Promise.resolve(false));
  const before3 = FETCHES.length;
  W.actions.saveConfig();
  await new Promise((r) => setTimeout(r, 10));
  req(FETCHES.length === before3, "声明保存取消后 fetch 次数为 0");
  req(nodeOf("c-text").value === '{"version":"9.9.9"}', "取消后编辑器内容原样保留");

  // AC-007 · 保存**失败**（这里让网络失败，走同一个 catch 分支）也保留输入。
  W.setConfirm(() => Promise.resolve(true));
  const typed = '{"version":"9.9.9","declared_by":"qa"}\n// 用户正在改的内容';
  nodeOf("c-text").value = typed;
  const before4 = FETCHES.length;
  W.actions.saveConfig();
  await new Promise((r) => setTimeout(r, 10));
  req(FETCHES.length === before4 + 1, "确认后确实发了 1 次 PUT");
  req(nodeOf("c-text").value === typed, "保存失败后编辑器内容**未被清空 / 回滚**（AC-007）");
  const saveErr = nodeOf("c-out").textContent;
  req(saveErr.indexOf("NETWORK") >= 0, "失败时显示 code（NETWORK）", saveErr);
  req(saveErr.indexOf("无法连接") >= 0, "失败时显示可读原因（不是「失败」两个字）", saveErr);
  req(saveErr.indexOf("怎么办") >= 0, "失败时显示 hint", saveErr);

  // AC-009 · 停服 / 网络失败要给出可读原因，而不是「加载失败」四个字。
  nodeOf("p-apply-out").textContent = "";
  W.setConfirm(() => Promise.resolve(true));
  W.actions.apply();
  await new Promise((r) => setTimeout(r, 10));
  const netErr = nodeOf("p-apply-out").textContent;
  req(netErr.indexOf("无法连接服务") >= 0, "网络失败显示「无法连接服务」+ 具体原因（AC-009）", netErr);
  req(netErr.indexOf("同源") >= 0, "网络失败提示了 CSP / 同源这条排查路径", netErr);

  // ── AC-010 · XSS：用户数据只进 textContent ──────────────────
  console.log("\n[AC-010] 含 <img src=x onerror=alert(1)> 的对象名必须显示原文");
  const evil = '<img src=x onerror=alert(1)>';
  const node = W.el("div", null, evil);
  req(node.textContent === evil, "el() 把危险字符串原样放进 textContent", node.textContent);
  const table = W.makeTable(["对象名"], [[evil]], "空");
  const textAll = table.textContent;
  req(textAll.indexOf(evil) >= 0, "表格单元格里是原文（不是被解析的 HTML）", textAll);
  req(innerHtmlWrites === 0, "整个过程 innerHTML 赋值次数 = 0", "实际 " + innerHtmlWrites + " 次");

  // ── AC-003 · 口令只进 sessionStorage ───────────────────────
  console.log("\n[AC-003] 口令只进 sessionStorage");
  W.state.at = "PROBE-ADMIN";
  W.remember();
  req(sandbox.sessionStorage.getItem("ontok.console.admin") === "PROBE-ADMIN", "admin 口令进了 sessionStorage");
  req(
    Array.from(sandbox.sessionStorage._map.keys()).join(",") === "ontok.console.admin",
    "sessionStorage 里只有 admin 一把口令（REQ-161b：查询口令那条旁路已删）",
    Array.from(sandbox.sessionStorage._map.keys()).join(",")
  );
  req(typeof sandbox.localStorage === "undefined", "沙箱里没有 localStorage（页面也没引用它）");
  req(!/localStorage/.test(SCRIPT), "页面脚本里 localStorage 出现 0 次");
  req(!/indexedDB/.test(SCRIPT) && !/document\.cookie/.test(SCRIPT), "页面脚本里没有 indexedDB / document.cookie");
  req(!/location\.search/.test(SCRIPT) && !/location\.hash/.test(SCRIPT), "口令不进 URL（不读 location.search / hash）");

  // ── 普通请求的 401/403：清口令 + 回登录态（安全语义没被改坏）──
  // （REQ-161b 之后本页只剩管理端点，所以这条回归现在覆盖**全部**请求；
  //  原先与它并列的「查询端点不清口令」那一组已随查询台删除。）
  console.log("\n[B-1 回归] 管理端点的 401/403 仍然清口令 + 回登录态");
  const FORBIDDEN_BODY = JSON.stringify({
    code: "TOKEN_KIND_FORBIDDEN",
    message: "这是一张 query token，不能调用管理端点。",
    hint: "管理端点只接受 kind = \"admin\" 的 token"
  });

  nodeOf("login").hidden = true;
  sandbox.sessionStorage.setItem("ontok.console.admin", "ADMIN-KEEP");
  W.state.at = "ADMIN-KEEP";
  sandbox.__reply(401, JSON.stringify({ code: "UNAUTHORIZED", message: "没有可用的访问口令。", hint: "" }));
  let e401 = null;
  try {
    await W.request("api/projects");
  } catch (e) {
    e401 = e;
  }
  req(!!e401, "管理端点 401 抛出错误");
  req(W.state.at === "", "管理端点 401 ⇒ 内存里的口令被清空");
  req(sandbox.sessionStorage.getItem("ontok.console.admin") === null, "管理端点 401 ⇒ sessionStorage 的 admin 被删");
  req(nodeOf("login").hidden === false, "管理端点 401 ⇒ 回到登录态（登录框可见）");
  req(nodeOf("msg").textContent.indexOf("口令无效或权限不够") >= 0, "管理端点 401 ⇒ 显示原因", nodeOf("msg").textContent);
  req(nodeOf("token").value === "", "管理端点 401 ⇒ 口令输入框被清空");

  nodeOf("login").hidden = true;
  W.state.at = "ADMIN-KEEP";
  sandbox.__reply(403, FORBIDDEN_BODY);
  let e403 = null;
  try {
    await W.request("api/projects");
  } catch (e) {
    e403 = e;
  }
  req(!!e403, "管理端点 403 抛出错误");
  req(W.state.at === "" && nodeOf("login").hidden === false, "管理端点 403 ⇒ 同样清口令 + 回登录态");

  // ── REQ-161b · 构建包的门禁口径还在 + 查询服务的残留 = 0 ─────
  console.log("\n[REQ-161b] 构建包口径仍在；查询台 / 目录浏览的残留 = 0");
  for (const needle of ["progress.pending", "already_imported", "reveal_key", "dsnFlag(", "countSummary"]) {
    req(SCRIPT.indexOf(needle) >= 0, "构建包口径仍在：" + needle);
  }
  // 5 处危险动作：`function guarded(spec, run)` 不命中 `guarded({`。
  const guardedCalls = (SCRIPT.match(/guarded\(\{/g) || []).length;
  req(guardedCalls === 5, "危险动作恰好 5 处过 guarded（apply / freeze / 声明保存 / 激活 / 换 key）", "实际 " + guardedCalls + " 处");
  req(/\["目标"/.test(SCRIPT) && /\["后果"/.test(SCRIPT), "确认框都写了目标 + 后果");
  // 被删的两个视图不许有半套残留（小写路径词根；测试文件里点名，源码里不该出现）。
  const leftovers = ["sparql", "catalog", "describe", "drawer"].filter((w) => SCRIPT.indexOf(w) >= 0);
  req(leftovers.length === 0, "页面脚本里查询服务的残留词 = 0", "命中：" + leftovers.join(", "));
  req(!/id="k-|id="q-|id="qtoken"/.test(HTML), "被删视图的 DOM id（k-*/q-*/qtoken）不存在");
  req(!/id="drawer"|id="v-query"|id="v-catalog"/.test(HTML), "被删的视图容器 / 抽屉 DOM 不存在");
  req(HTML.indexOf("function metaLines(") < 0, "查询台的元信息渲染点已删（metaLines）");
  req(HTML.indexOf("opts.query") < 0, "查询口令旁路（opts.query）已删");
  const tabs = Array.isArray(W.tabs) ? W.tabs : [];
  req(tabs.length === 6, "导航恰好 6 项", "实际 " + tabs.length + " 项：" + tabs.join("/"));
  req(tabs[1] === "bundles", "构建包在 02 位（用户每天第一件事：上传包）", tabs.join("/"));

  // ── 报告 ────────────────────────────────────────────────────
  console.log("");
  if (failures.length) {
    console.error("失败 " + failures.length + " 条（通过 " + pass + " 条）：\n- " + failures.join("\n- "));
    process.exit(1);
  }
  console.log("全部通过：" + pass + " 条判据。");
})().catch((e) => {
  console.error("探针自身出错：" + (e && e.stack ? e.stack : e));
  process.exit(1);
});
