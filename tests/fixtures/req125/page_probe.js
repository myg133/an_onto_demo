#!/usr/bin/env node
// REQ-125 · 工作台页面壳的**交互层判据**（Node + 假 DOM；本机没有 headless 浏览器）。
//
// 它把 `src/console/page.rs` 里的 `<script>` **原样**抽出来，在一个最小 DOM 桩上跑，
// 于是下面四条判据验的是**线上那一份代码**，不是另写一遍的模拟：
//
// | 判据 | 落点 |
// | --- | --- |
// | AC-008 | `metaLines(回执)` 产出 warnings / coverage(main_only) / join_states(used=false) / truncated |
// | AC-006 | 取消确认框 ⇒ **一次 fetch 都不发**；确认后才发 |
// | AC-010 | 把 `<img src=x onerror=alert(1)>` 交给渲染器 ⇒ 只进 `textContent`；假 DOM 的 `innerHTML` 赋值即抛 |
// | AC-003 | `remember()` 只写 sessionStorage；沙箱里**没有** localStorage（碰了就 ReferenceError） |
//
// 跑法：
//   node tests/fixtures/req125/page_probe.js                     # 内置样本
//   node tests/fixtures/req125/page_probe.js receipt.json        # 用真实 /api/sparql 回执
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
    return Promise.reject(new TypeError("probe: network disabled"));
  }
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
function includesAll(lines, needles, label) {
  const text = lines.map((l) => l.text).join("\n");
  const missing = needles.filter((n) => text.indexOf(n) < 0);
  ok(missing.length === 0, label, missing.length ? ("缺：" + missing.join(" / ") + "\n      " + text) : "");
  return text;
}

(async function main() {
  console.log("REQ-125 · 工作台页面探针（假 DOM，跑页面里那一份 JS）\n");

  // ── AC-008 · 查询台四段元信息 ──────────────────────────────
  console.log("[AC-008] 查询台必须显示 warnings / coverage / join_states(used=false) / truncated");
  const receiptPath = process.argv[2];
  const receipt = receiptPath
    ? JSON.parse(fs.readFileSync(receiptPath, "utf8"))
    : {
        columns: ["customer_id", "amount"],
        rows: [["C0001", "12.30"]],
        row_count: 1,
        truncated: true,
        warnings: [
          "表 deductions 的覆盖面是 main_only：子账号产生的扣费不在结果内，金额可能偏低"
        ],
        coverage: [
          { table: "deductions", coverage: "main_only" },
          { table: "customers", coverage: "complete" }
        ],
        join_states: [
          { from: "deductions", to: "customers", predicate: "deductions__customers", used: false, state: "true", evidence: "value-overlap 4/4" },
          { from: "balances", to: "customers", predicate: "balances__customers", used: true, state: "true", evidence: "value-overlap 5/5" }
        ],
        join_states_scope: "all_edges_between_involved_tables",
        join_states_scope_note: "join_states[] 是涉及的表之间的全部边，不是实走边。",
        limits: { max_rows: 200, token_max_rows: 200, timeout_ms: 10000 },
        tables: ["deductions", "customers"],
        tenant_applied: [{ table: "deductions", column: "customer_id", value: "C0001" }],
        public_dimensions: []
      };

  const lines = Array.from(W.metaLines(receipt));
  if (process.argv.indexOf("--dump") >= 0) {
    // 只打印「页面会显示的那几行」——给人看的现场证据（不跑断言）。
    console.log("metaLines(回执) =>");
    lines.forEach((l, i) => console.log("  " + (i + 1) + ". [" + (l.cls || "info") + "] " + l.text));
    process.exit(0);
  }
  req(lines.length >= 4, "metaLines 至少产出 4 行", "实际 " + lines.length + " 行");
  const text = lines.map((l) => l.text).join("\n");
  const hasWarnings = (receipt.warnings || []).length > 0;
  const hasMainOnly = (receipt.coverage || []).some((c) => c.coverage === "main_only");
  const unused = (receipt.join_states || []).filter((j) => j.used === false);
  if (hasWarnings) {
    includesAll(lines, ["可能漏数据", "warnings"], "warnings 文案可见（AC-008 第一条）");
  } else {
    includesAll(lines, ["warnings：0 条"], "没有 warning 时明确写「0 条」（AC-008 第一条）");
  }
  if (hasMainOnly) {
    const mainOnlyTables = (receipt.coverage || []).filter((c) => c.coverage === "main_only").map((c) => c.table);
    req(text.indexOf("main_only") >= 0, "coverage 的 main_only 可见（AC-008 第二条）");
    for (const t of mainOnlyTables) {
      req(text.indexOf(t) >= 0, "main_only 的表名可见：" + t);
    }
  } else {
    req(text.indexOf("main_only") < 0, "没有 main_only 就不该出现 main_only 文案");
  }
  if (unused.length) {
    req(text.indexOf("used=false") >= 0, "join_states 里 used=false 的边被点名（AC-008 第三条）");
    req(text.indexOf("没有走到") >= 0, "used=false 的语义说清了（涉及但未用）");
    for (const j of unused) {
      req(text.indexOf(j.from + " → " + j.to) >= 0, "未用边可见：" + j.from + " → " + j.to);
    }
  } else {
    includesAll(lines, ["used=false：0 条"], "没有未用边时明确写「0 条」（AC-008 第三条）");
  }
  includesAll(
    lines,
    [receipt.truncated ? "truncated：是" : "truncated：否"],
    "truncated 逐字显示（AC-008 第四条）"
  );
  includesAll(lines, ["limits", "timeout_ms"], "limits 一并显示（max_rows 是谁压的很清楚）");
  // ⛔ 只显示表格：把表格数据藏掉后，警告仍在（元信息不依赖 rows）。
  const noRows = Object.assign({}, receipt, { rows: [], row_count: 0, columns: [] });
  const lines2 = Array.from(W.metaLines(noRows));
  includesAll(lines2, ["可能漏数据"], "0 行时警告仍然显示（不是「没数据就没风险」）");
  // 干净回执也必须明确说「0 条」，不能静默。
  const clean = Object.assign({}, receipt, {
    warnings: [], coverage: [{ table: "customers", coverage: "complete" }],
    join_states: [], truncated: false
  });
  const lines3 = Array.from(W.metaLines(clean));
  includesAll(lines3, ["warnings：0 条", "used=false：0 条", "truncated：否"], "没有风险时逐项说「0 / 否」");

  // ── AC-006 · 取消确认 ⇒ 不发请求 ───────────────────────────
  console.log("\n[AC-006] 危险操作二次确认：取消不发请求");
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
  W.state.qt = "PROBE-QUERY";
  W.remember();
  req(sandbox.sessionStorage.getItem("ontok.console.admin") === "PROBE-ADMIN", "admin 口令进了 sessionStorage");
  req(sandbox.sessionStorage.getItem("ontok.console.query") === "PROBE-QUERY", "query 口令进了 sessionStorage");
  req(typeof sandbox.localStorage === "undefined", "沙箱里没有 localStorage（页面也没引用它）");
  req(!/localStorage/.test(SCRIPT), "页面脚本里 localStorage 出现 0 次");

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
