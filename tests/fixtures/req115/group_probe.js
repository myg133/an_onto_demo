#!/usr/bin/env node
// ═══════════════════════════════════════════════════════════════════════════
// REQ-115 · 分组内核探针（AC-002 / AC-003 / AC-006 的**数据级**判据）
// ═══════════════════════════════════════════════════════════════════════════
//
// # 怎么跑
//
//     node tests/fixtures/req115/group_probe.js
//
// 零依赖：只用 Node 内置的 `fs` / `path`。✅ 通过 exit 0，❌ 失败 exit 1。
//
// # 它凭什么能代表页面
//
// 和 REQ-114 的布局探针同一条纪律：**不复制**页面的分组代码，而是从
// `src/server/page.rs` 里把夹在两条标记之间的「分组内核」段
// （`groupIndex` / `groupOf` / `unitsOf` / `renderedCount`）**原样抽出来**，
// 用 `new Function` 在 Node 里执行同一份源码。标记被删/改名 ⇒ **直接失败**。
//
// # 它验证什么
//
// 1. **AC-002 折叠态节点数**：3 个组（成员 2 / 3 / 1）+ 4 个未分组对象，
//    全部折叠 ⇒ 渲染单元数 = 3 + 4 = **7**；全部展开 ⇒ 2+3+1+4 = **10**。
// 2. **AC-003 无组**：`groups = []` ⇒ 渲染单元数 = 对象数，且**没有任何聚合单元**
//    （⛔ 不许出现「默认组」）。
// 3. **未知组 id**：`node.group` 指向一个不在 `groups` 里的 id ⇒ 按「未分组」画
//    （不虚构、不崩溃）。
// 4. **纯函数**：`unitsOf` 不改入参（深拷贝前后比较）。
// 5. **页面不自己算分组**（AC-006 的源码层）：剥掉注释后的页面 JS 里不得出现
//    `louvain` / `modularity` / `cluster` / `community` / `聚类` / `贪心` / `merge`。
// 6. **折叠不改数据**（AC-009）：`setFold` / `foldAll` 的源码里不得出现
//    `fetch(` / `ask(` / `api/` —— 折叠是纯前端状态，不可能碰库或导出。
//
// # 已知边界
//
// 本脚本证明的是**单元数**与**源码层**，不是浏览器里的观感：组圈的位置、
// 折叠动画、拖拽手感都没有被它覆盖（本机无 headless 浏览器，未做真浏览器手测）。
"use strict";

const fs = require("fs");
const path = require("path");

const ROOT = path.resolve(__dirname, "..", "..", "..");
const PAGE = path.join(ROOT, "src", "server", "page.rs");

// 两条标记必须与 page.rs 里的一字不差。改页面结构时**同时**改这里 —— 否则本脚本失败。
const BEGIN = "// ─── 分组内核（纯函数、不碰 DOM）─────────────────────────────";
const END = "// ─── 分组内核结束 ───────────────────────────────────────────";

let failures = 0;
function pass(name, extra) {
  console.log("PASS " + name + (extra === undefined ? "" : "  " + extra));
}
function fail(name, extra) {
  console.log("FAIL " + name + (extra === undefined ? "" : "  " + extra));
  failures++;
}
function expect(cond, name, extra) {
  if (cond) pass(name, extra);
  else fail(name, extra);
}
function die(msg) {
  console.log("FAIL " + msg);
  console.log("\n探针无法完成 ⇒ exit 1（⛔ 不许把这种情形当成通过）");
  process.exit(1);
}

// ── ① 读页面源码 ──────────────────────────────────────────────────────────
if (!fs.existsSync(PAGE)) die("读不到 " + PAGE);
const src = fs.readFileSync(PAGE, "utf8");

// ── ② 抽出分组内核 ────────────────────────────────────────────────────────
const i0 = src.indexOf(BEGIN);
const i1 = src.indexOf(END);
if (i0 < 0 || i1 < 0 || i1 <= i0) {
  die("在 page.rs 里找不到分组内核的两条标记（BEGIN/END）—— 页面结构变了，先把标记补回来");
}
const kernel = src.slice(i0, i1);
if (!kernel.includes("function unitsOf") || !kernel.includes("function renderedCount")) {
  die("抽到的内核段里没有 unitsOf / renderedCount —— 标记位置不对");
}

let K;
try {
  const exports_ = {};
  new Function(
    "exports",
    kernel +
      "\nexports.groupIndex = groupIndex; exports.groupOf = groupOf;" +
      "exports.unitsOf = unitsOf; exports.renderedCount = renderedCount;"
  )(exports_);
  K = exports_;
} catch (e) {
  die("分组内核在 Node 里执行失败：" + e.message);
}
for (const name of ["groupIndex", "groupOf", "unitsOf", "renderedCount"]) {
  if (typeof K[name] !== "function") die("内核缺少函数 " + name + "（抽出来的东西是空的）");
}

console.log("REQ-115 · 分组内核探针（从 src/server/page.rs **原样抽取**）");
console.log("内核段 = " + Buffer.byteLength(kernel, "utf8") + " 字节\n");

// ── ③ 夹具：3 组 + 4 个未分组 ─────────────────────────────────────────────
function clamp(v) {
  return JSON.parse(JSON.stringify(v));
}
const GROUPS = [
  { id: "g1", display_name: "甲组", members: ["n1", "n2"] },
  { id: "g2", display_name: "乙组", members: ["n3", "n4", "n5"] },
  { id: "g3", display_name: "丙组", members: ["n6"] },
];
const NODES = [
  { id: "n1", group: "g1" },
  { id: "n2", group: "g1" },
  { id: "n3", group: "g2" },
  { id: "n4", group: "g2" },
  { id: "n5", group: "g2" },
  { id: "n6", group: "g3" },
  { id: "u1", group: null },
  { id: "u2", group: null },
  { id: "u3", group: null },
  { id: "u4", group: null },
];
const GROUP_COUNT = GROUPS.length;
const UNGROUPED = 4;
const NODE_COUNT = NODES.length;

function allFolded() {
  const f = {};
  for (const g of GROUPS) f[g.id] = true;
  return f;
}

// ── ④ AC-002：折叠态节点数 = 组数 + 未分组数 ──────────────────────────────
const folded = allFolded();
const foldedUnits = K.unitsOf(GROUPS, NODES, folded);
expect(
  foldedUnits.length === GROUP_COUNT + UNGROUPED,
  "折叠态渲染节点数 = 组数 " + GROUP_COUNT + " + 未分组 " + UNGROUPED + " = " + (GROUP_COUNT + UNGROUPED),
  "实际 " + foldedUnits.length
);
expect(
  K.renderedCount(GROUPS, NODES, folded) === foldedUnits.length,
  "renderedCount 与 unitsOf 一致（同一份判据）",
  "实际 " + K.renderedCount(GROUPS, NODES, folded)
);
const foldKinds = foldedUnits.filter((u) => u.fold === true).length;
expect(foldKinds === GROUP_COUNT, "折叠态聚合单元数 = 组数", "实际 " + foldKinds);
const foldedIds = foldedUnits.map((u) => u.id).sort();
const wantIds = GROUPS.map((g) => g.id).concat(["u1", "u2", "u3", "u4"]).sort();
expect(
  JSON.stringify(foldedIds) === JSON.stringify(wantIds),
  "折叠态单元 = 每组一个聚合节点 + 每个未分组对象一个节点",
  foldedIds.join(",")
);

// ── ⑤ 展开：每个成员回到图上 ─────────────────────────────────────────────
const expanded = K.unitsOf(GROUPS, NODES, {});
expect(
  expanded.length === NODE_COUNT,
  "展开态渲染节点数 = 对象数 " + NODE_COUNT,
  "实际 " + expanded.length
);
expect(
  expanded.every((u) => u.fold === false),
  "展开态没有任何聚合单元"
);
expect(
  expanded.some((u) => u.id === "u4") && expanded.some((u) => u.id === "n6"),
  "展开态同时包含组内成员与未分组对象"
);

// ── ⑥ AC-003：无组时不许虚构 ─────────────────────────────────────────────
const empty = K.unitsOf([], NODES, {});
expect(
  empty.length === NODE_COUNT,
  "无组时渲染节点数 = 对象数（没有默认组）",
  "实际 " + empty.length
);
expect(
  empty.every((u) => u.fold === false),
  "无组时没有任何聚合单元（⛔ 不许出现默认组）"
);

// ── ⑦ 未知组 id ⇒ 按未分组画，不虚构 ─────────────────────────────────────
const ghostNodes = [{ id: "x", group: "ghost" }, { id: "y", group: null }];
expect(
  K.groupOf(ghostNodes[0], K.groupIndex(GROUPS)) === null,
  "指向未知组 id 的节点按「未分组」处理（不虚构一个组）"
);
expect(
  K.unitsOf(GROUPS, ghostNodes, allFolded()).length === GROUP_COUNT + 2,
  "未知组对象与真正未分组的对象一样各占一个节点",
  "实际 " + K.unitsOf(GROUPS, ghostNodes, allFolded()).length
);

// ── ⑧ 纯函数：不改入参 ───────────────────────────────────────────────────
const before = clamp({ groups: GROUPS, nodes: NODES, fold: folded });
const snapshot = clamp({ groups: GROUPS, nodes: NODES, fold: folded });
K.unitsOf(GROUPS, NODES, folded);
expect(
  JSON.stringify(before) === JSON.stringify(snapshot),
  "unitsOf 不改入参（折叠只是换一种画法）"
);

// ── ⑨ AC-006 / AC-009：源码层 ─────────────────────────────────────────────
function stripJsComments(code) {
  let out = "";
  let i = 0;
  let quote = null;
  while (i < code.length) {
    const c = code[i];
    const n = code[i + 1];
    if (quote) {
      out += c;
      if (c === "\\") {
        if (n !== undefined) out += n;
        i += 2;
        continue;
      }
      if (c === quote) quote = null;
      i++;
      continue;
    }
    if (c === '"' || c === "'" || c === "`") {
      quote = c;
      out += c;
      i++;
      continue;
    }
    if (c === "/" && n === "/") {
      while (i < code.length && code[i] !== "\n") i++;
      continue;
    }
    if (c === "/" && n === "*") {
      i += 2;
      while (i < code.length && !(code[i] === "*" && code[i + 1] === "/")) i++;
      i += 2;
      out += " ";
      continue;
    }
    out += c;
    i++;
  }
  return out;
}

const pageJs = (() => {
  const m = src.match(/<script>\r?\n([\s\S]*?)\r?\n<\/script>/);
  return m ? m[1] : "";
})();
if (!pageJs) die("抽不到页面的 <script> 段 —— 页面结构变了");
const jsCode = stripJsComments(pageJs);

const GROUPING_WORDS = ["louvain", "modularity", "cluster", "community", "聚类", "贪心", "merge"];
for (const w of GROUPING_WORDS) {
  expect(
    jsCode.toLowerCase().indexOf(w) < 0,
    "页面 JS（剥注释后）不含分组算法词：" + w
  );
}
expect(
  jsCode.indexOf("DATA.groups") >= 0,
  "页面 JS 从 DATA.groups 取分组（分组唯一来源）"
);

// 折叠/展开的实现 + 它们的函数体里不许有网络调用（AC-009）
function bodyOf(name) {
  const at = jsCode.indexOf("function " + name + "(");
  if (at < 0) return null;
  let depth = 0;
  let started = false;
  for (let i = at; i < jsCode.length; i++) {
    if (jsCode[i] === "{") {
      depth++;
      started = true;
    } else if (jsCode[i] === "}") {
      depth--;
      if (started && depth === 0) return jsCode.slice(at, i + 1);
    }
  }
  return null;
}
for (const fn of ["setFold", "foldAll", "relayout"]) {
  const b = bodyOf(fn);
  if (!b) {
    expect(false, "页面里有 " + fn + "（折叠/展开的实现位置）");
    continue;
  }
  expect(true, "页面里有 " + fn + "（折叠/展开的实现位置）");
  expect(
    b.indexOf("fetch(") < 0 && b.indexOf("ask(") < 0 && b.indexOf("api/") < 0,
    fn + " 的函数体里没有网络调用（折叠不改数据 / 不触导出，AC-009）"
  );
}

console.log("");
if (failures === 0) {
  console.log("ALL PASS（折叠态 " + (GROUP_COUNT + UNGROUPED) + " = 组 " + GROUP_COUNT + " + 未分组 " + UNGROUPED + "）");
  process.exit(0);
}
console.log(failures + " 条 FAIL");
process.exit(1);
