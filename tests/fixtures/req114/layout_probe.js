#!/usr/bin/env node
// ═══════════════════════════════════════════════════════════════════════════
// REQ-114 · 布局内核探针（AC-002 的**数据级**判据，入库、可复跑、**有牙**）
// ═══════════════════════════════════════════════════════════════════════════
//
// # 怎么跑
//
//     node tests/fixtures/req114/layout_probe.js
//
// 零依赖：只用 Node 内置的 `fs` / `path`。✅ 通过 exit 0，❌ 失败 exit 1
// ⇒ 可以直接当判据用（CI 里 `node tests/fixtures/req114/layout_probe.js`）。
//
// # 它验证什么（三层，缺一层就会出现假绿）
//
// **① 静态层（剥注释后数）**：内核里 `collide(ns, GAP);` 必须**恰好 2 处**。
//    ⚠️ 先剥掉 `//` 与 `/* *​/` 注释再数 —— 直接数原文的话，把两行注释掉照样能数到 2 处
//    （QA 实测过：这是本探针第一版的假绿根因）。
//    这也意味着「改成 3 遍」要**同时**改本文件：这个 2 是刻意绑定的，不是巧合。
//
// **② 单元层**：直接调 `collide()` —— 重叠的一对被推到「半径和 + GAP」；
//    钉住的节点不许动（否则拖拽会被下一帧弹回去）。
//
// **③ 涌现层（两个场景）**：
//    - **场景 1（真实参数）**：83 个节点全部同坐标，`REP=400` 如页面默认，推进 300 帧上限，
//      断言「任意两节点中心距 ≥ 半径和」；
//    - **场景 2（无斥力）**：同样 83 个节点同坐标，但把 `REP` 置 **0** ⇒ **节点只能靠
//      collide 分开**。这一格是给「collide 被注释掉 / 被包进 `if (false)`」准备的：
//      只要 collide 真的没执行，节点就会全叠在原点，余量必然 ≪ 0。
//      （只跑场景 1 抓不住这类变异：斥力很强，把 collide 删掉仍能凑出 ~12px 余量，已实测。）
//
// # 它凭什么能代表页面
//
// 本脚本**不复制**页面的布局代码：它从 `src/server/page.rs` 里把夹在两条标记
// 之间的「布局内核」段（`radiusOf` / `collide` / `springs` / `physicsStep` 及其常量）
// **原样抽出来**，用 `new Function` 在 Node 里执行同一份源码。
// ⇒ 页面改了内核而探针没跑，这里就会给出不同的数字；标记被删/改名则**直接失败**
//    （⛔ 绝不「找不到就 return 0」—— 那是假绿）。
//
// 唯一的注入点是 `setRep()`：它改的是内核作用域里那个 `REP` 变量本身（不是复制一份
// 参数），所以「无斥力」场景跑的还是页面那条 `physicsStep`。
//
// # 期望输出（本机实测；内核里没有 Math.random，数字确定）
//
//     场景 1（REP=400）：168 帧，最紧余量 = +15.79px，包围盒 529 × 528
//     场景 2（REP=0）  ：168 帧，最紧余量 = +5.20px，包围盒 288 × 305
//     末行 `ALL PASS（…）`，exit 0
//
// # 为什么帧数是 168 而不是 300
//
// 页面里 `alpha` 每帧 ×0.97，`0.97^168 ≈ 0.006`（= `ALPHA_MIN`）⇒ 先撞上「收敛」
// 那条闸就停帧。这正是「力导向必须收敛、不许永远满帧」的证据。
//
// # 变异自检（每次改探针都该跑一遍，QA 也会跑）
//
// | 变异 | 手法 | 期望 |
// | --- | --- | --- |
// | 基线 | 不动 | exit 0 |
// | M1a | 两行 `collide(ns, GAP);` 删**一行** | exit 1（静态层 1 ≠ 2） |
// | M1c | 两行改成 `//` 注释 | exit 1（剥注释后 0 ≠ 2；且场景 2 余量 -32） |
// | M1d | 两行包进 `if (false) { … }` | exit 1（静态层仍 2 ⇒ 靠**场景 2** 抓住，余量 -32） |
// | M2 | `var min = a.r + b.r + gap;` → `var min = 0;` | exit 1（单元层 + 场景 2） |
// | M3 | 删内核段 / 改 `END` 标记 | exit 1（`die()`，不是静默通过） |
//
// ⚠️ 边界：本脚本证明的是**布局算法**，不是**浏览器里的观感**。拖拽手感、标签避让的
//    视觉效果、暗色配色都没有被它覆盖（本机无 headless 浏览器，未做真浏览器手测）。
"use strict";

const fs = require("fs");
const path = require("path");

const ROOT = path.resolve(__dirname, "..", "..", "..");
const PAGE = path.join(ROOT, "src", "server", "page.rs");

// 两条标记必须与 page.rs 里的一字不差。改页面结构时**同时**改这里 —— 否则本脚本失败。
const BEGIN = "// ─── 布局内核（纯函数、不碰 DOM）─────────────────────────────";
const END = "// ─── 布局内核结束 ───────────────────────────────────────────";

const N_NODES = 83; // 真实冻结版 anspire_real v2 的节点数
const N_EDGES = 52; // 真实冻结版 anspire_real v2 的边数
const MAX_FRAMES = 300; // 页面硬约束：力导向最多 300 帧
const WANT_COLLIDE_CALLS = 2; // 每帧两遍碰撞消解（与 page.rs 的 physicsStep 绑定）

let failures = 0;

function pass(name, extra) {
  console.log("PASS " + name + (extra === undefined ? "" : "  " + extra));
}
function fail(name, extra) {
  console.log("FAIL " + name + (extra === undefined ? "" : "  " + extra));
  failures++;
}
// 断言：条件为假 ⇒ 记一条 FAIL（不是「打印一句话」）。
// ⛔ 别再写 pass(name, cond)：那会把条件变成一句打印出来的字，探针永远不会红。
function expect(cond, name, extra) {
  if (cond) pass(name, extra);
  else fail(name, extra);
}
function die(msg) {
  // 「读不到 / 抽不到」必须**直接失败**，不能静默通过。
  console.log("FAIL " + msg);
  console.log("\n探针无法完成 ⇒ exit 1（⛔ 不许把这种情形当成通过）");
  process.exit(1);
}
function fmt(n, digits) {
  return Number(n).toFixed(digits === undefined ? 2 : digits);
}

// ── ① 读页面源码 ──────────────────────────────────────────────────────────
if (!fs.existsSync(PAGE)) die("读不到 " + PAGE);
const src = fs.readFileSync(PAGE, "utf8");

// ── ② 抽出布局内核（夹在两条标记之间）────────────────────────────────────
const i0 = src.indexOf(BEGIN);
const i1 = src.indexOf(END);
if (i0 < 0 || i1 < 0 || i1 <= i0) {
  die("在 page.rs 里找不到布局内核的两条标记（BEGIN/END）—— 页面结构变了，先把标记补回来");
}
const kernel = src.slice(i0, i1);
if (!kernel.includes("function physicsStep") || !kernel.includes("function collide")) {
  die("抽到的内核段里没有 physicsStep / collide —— 标记位置不对");
}

// ── ③ 静态层：**剥注释后**数 collide 调用，要求恰好 2 处 ─────────────────
// 为什么必须剥注释：直接数原文时，`// collide(ns, GAP);` 与 `/* collide … *​/`
// 照样被数到 —— 那正是本探针第一版被 QA 判「无牙、假绿」的原因。
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
      out += " "; // 留一个分隔符，别把前后两个词粘成一个
      continue;
    }
    out += c;
    i++;
  }
  return out;
}

const stripped = stripJsComments(kernel);
const calls = (stripped.match(/collide\(ns, GAP\);/g) || []).length;

console.log("REQ-114 · 布局内核探针（从 src/server/page.rs **原样抽取**）");
console.log("内核段 = " + Buffer.byteLength(kernel, "utf8") + " 字节" +
  "（radiusOf / collide / springs / physicsStep）");
console.log("剥注释后内核 = " + Buffer.byteLength(stripped, "utf8") + " 字节\n");

// 反空转：剥注释这一步本身不许把代码剥没了
expect(Buffer.byteLength(stripped, "utf8") > Buffer.byteLength(kernel, "utf8") / 2,
  "剥注释没有把代码剥没（自检）",
  Buffer.byteLength(stripped, "utf8") + " / " + Buffer.byteLength(kernel, "utf8") + " 字节");
expect(calls === WANT_COLLIDE_CALLS,
  "静态：剥注释后 collide(ns, GAP); 恰好 " + WANT_COLLIDE_CALLS + " 处",
  "实际 " + calls + " 处（注释/删行/多写都会在这里红）");

// ── ④ 在 Node 里执行同一份源码 ────────────────────────────────────────────
let K;
try {
  const exports_ = {};
  new Function(
    "exports",
    kernel +
      "\nexports.radiusOf = radiusOf; exports.collide = collide; exports.springs = springs;" +
      "exports.physicsStep = physicsStep; exports.GAP = GAP;" +
      "exports.SIM_FRAMES = SIM_FRAMES; exports.ALPHA_MIN = ALPHA_MIN;" +
      // 唯一的注入点：改的是内核作用域里的 `REP` 本身（不是复制一份参数）。
      "exports.setRep = function (v) { REP = v; };"
  )(exports_);
  K = exports_;
} catch (e) {
  die("布局内核在 Node 里执行失败：" + e.message);
}

// ── ⑤ 反空转：抽出来必须真的是一组可用的函数/常量 ────────────────────────
for (const name of ["radiusOf", "collide", "springs", "physicsStep", "setRep"]) {
  if (typeof K[name] !== "function") die("内核缺少函数 " + name + "（抽出来的东西是空的）");
}
for (const name of ["GAP", "SIM_FRAMES", "ALPHA_MIN"]) {
  if (typeof K[name] !== "number" || !isFinite(K[name])) die("内核缺少常量 " + name);
}
expect(K.SIM_FRAMES === MAX_FRAMES, "页面帧上限 SIM_FRAMES === " + MAX_FRAMES, "实际 " + K.SIM_FRAMES);

// ── ⑥ 造图：83 节点（坐标全 0）+ 52 条边 + 所属连线 ──────────────────────
function buildNodes() {
  const ns = [];
  for (let i = 0; i < N_NODES; i++) {
    const attr = (i * 7) % 12; // 属性数 0..11，半径随之变化（6 → 16）
    ns.push({
      id: "n" + i,
      label: "对象" + i,
      c: attr,
      r: K.radiusOf(attr),
      x: 0,
      y: 0,
      vx: 0,
      vy: 0,
      pin: false,
    });
  }
  return ns;
}

function buildEdges(ns) {
  const es = [];
  let i = 0;
  while (es.length < N_EDGES && i < N_EDGES * 50) {
    const a = ns[i % ns.length];
    const b = ns[(i * 13 + 7) % ns.length];
    const dup = es.some((e) => (e.a === a && e.b === b) || (e.a === b && e.b === a));
    if (a !== b && !dup) es.push({ a: a, b: b, label: "关系" + i });
    i++;
  }
  return es;
}

// 所属连线（真实数据里 parent 存在时的弱弹簧，一并带上）
function buildParents(ns) {
  const ps = [];
  for (let i = 1; i < ns.length; i += 9) ps.push({ a: ns[i - 1], b: ns[i] });
  return ps;
}

function minSlack(ns) {
  let worst = Infinity;
  let pair = null;
  for (let i = 0; i < ns.length; i++) {
    for (let j = i + 1; j < ns.length; j++) {
      const a = ns[i];
      const b = ns[j];
      const dist = Math.hypot(b.x - a.x, b.y - a.y);
      const need = a.r + b.r;
      if (dist - need < worst) {
        worst = dist - need;
        pair = { i: i, j: j, dist: dist, need: need };
      }
    }
  }
  return { worst: worst, pair: pair };
}
function bboxOf(ns) {
  let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
  for (const d of ns) {
    x0 = Math.min(x0, d.x); x1 = Math.max(x1, d.x);
    y0 = Math.min(y0, d.y); y1 = Math.max(y1, d.y);
  }
  return { w: x1 - x0, h: y1 - y0 };
}
function countNaN(ns) {
  return ns.filter((d) => !isFinite(d.x) || !isFinite(d.y)).length;
}

// 与页面 loop() 同一套规则：alpha 每帧 ×0.97，撞闸即停。
function run(ns, es, ps) {
  let alpha = 1;
  let frames = 0;
  while (frames < MAX_FRAMES && alpha > K.ALPHA_MIN) {
    K.physicsStep(ns, es, ps, alpha);
    frames++;
    alpha = alpha * 0.97;
  }
  return { frames: frames, stoppedBy: alpha <= K.ALPHA_MIN ? "alpha 掉到 ALPHA_MIN 以下（收敛）" : "撞到 300 帧上限" };
}

// ── ⑦ 单元层：collide 本身（位置推离，不是斥力）─────────────────────────
const unitA = { x: 0, y: 0, r: 10, vx: 0, vy: 0, pin: false };
const unitB = { x: 4, y: 0, r: 12, vx: 0, vy: 0, pin: false };
K.collide([unitA, unitB], K.GAP);
const unitGap = Math.hypot(unitB.x - unitA.x, unitB.y - unitA.y) - (unitA.r + unitB.r);
expect(unitGap >= K.GAP - 1e-9, "单元：重叠的一对被推到「半径和 + GAP」",
  "余量 " + fmt(unitGap) + " ≥ GAP " + K.GAP);

const pinA = { x: 0, y: 0, r: 10, vx: 0, vy: 0, pin: true };
const pinB = { x: 3, y: 0, r: 10, vx: 0, vy: 0, pin: false };
K.collide([pinA, pinB], K.GAP);
expect(pinA.x === 0 && pinA.y === 0, "单元：钉住的节点不动（否则拖拽会被下一帧弹回去）",
  "(" + pinA.x + ", " + pinA.y + ")");
expect(Math.hypot(pinB.x - pinA.x, pinB.y - pinA.y) - (pinA.r + pinB.r) >= K.GAP - 1e-9,
  "单元：让对面的节点让开整个间距",
  "余量 " + fmt(Math.hypot(pinB.x - pinA.x, pinB.y - pinA.y) - (pinA.r + pinB.r)));

// ── ⑧ 场景 1：真实参数（REP=400），83 节点同坐标 ─────────────────────────
const nodes1 = buildNodes();
const edges1 = buildEdges(nodes1);
const parents1 = buildParents(nodes1);

expect(nodes1.every((d) => d.x === 0 && d.y === 0 && d.vx === 0 && d.vy === 0),
  "场景 1 起点：83 个节点都在 (0,0)，速度 0",
  nodes1.filter((d) => d.x === 0 && d.y === 0).length + "/" + N_NODES);
expect(edges1.length === N_EDGES, "场景 1 边数 === " + N_EDGES, "实际 " + edges1.length);

const r1 = run(nodes1, edges1, parents1);
const s1 = minSlack(nodes1);
const b1 = bboxOf(nodes1);
const n1 = countNaN(nodes1);

console.log("");
console.log("场景 1 · 真实参数（REP=400，页面默认）");
console.log("  节点数 / 边数        = " + nodes1.length + " / " + edges1.length);
console.log("  实际推进帧数         = " + r1.frames + "（上限 " + MAX_FRAMES + "，停止原因：" + r1.stoppedBy + "）");
console.log("  最紧一对（#" + s1.pair.i + " / #" + s1.pair.j + "）中心距 = " + fmt(s1.pair.dist) +
  "，半径和 = " + fmt(s1.pair.need));
console.log("  余量（距 − 半径和）  = " + fmt(s1.worst) + "   ← 断言 ≥ 0");
console.log("  含 NaN 的节点数      = " + n1 + "   ← 断言 = 0");
console.log("  包围盒（世界单位）   = " + fmt(b1.w, 0) + " × " + fmt(b1.h, 0));

expect(s1.worst >= 0, "场景 1（REP=400）：任意两节点中心距 ≥ 半径和", "最紧余量 " + fmt(s1.worst) + "px");
expect(n1 === 0, "场景 1：没有 NaN / 爆冲", n1 + " 个");
expect(r1.frames <= MAX_FRAMES, "场景 1：力导向收敛（帧数 ≤ " + MAX_FRAMES + "）", r1.frames + " 帧");
expect(r1.stoppedBy.indexOf("收敛") >= 0, "场景 1：不是靠满帧凑出来的（必须真的停）", r1.stoppedBy);
expect(b1.w > 100 && b1.h > 100, "场景 1：铺开后有实际尺寸（不是全缩在原点）",
  fmt(b1.w, 0) + " × " + fmt(b1.h, 0));

// ── ⑨ 场景 2：**无斥力**（REP=0），83 节点同坐标 ─────────────────────────
//
// 这一格是本次修复的核心：斥力归零后，「不重叠」这件事**只可能**由 collide 兑现。
// ⇒ collide 被删一处 / 被注释 / 被包进 if (false)，这里必然红（余量会掉到 −32 量级）。
const nodes2 = buildNodes();
const edges2 = buildEdges(nodes2);
const parents2 = buildParents(nodes2);
K.setRep(0); // ← 只改内核作用域里的 REP 本身，physicsStep 还是页面那一条
const r2 = run(nodes2, edges2, parents2);
const s2 = minSlack(nodes2);
const b2 = bboxOf(nodes2);
const n2 = countNaN(nodes2);

console.log("");
console.log("场景 2 · 无斥力（REP=0 ⇒ 只能靠 collide 分开）");
console.log("  节点数 / 边数        = " + nodes2.length + " / " + edges2.length);
console.log("  实际推进帧数         = " + r2.frames + "（停止原因：" + r2.stoppedBy + "）");
console.log("  最紧一对（#" + s2.pair.i + " / #" + s2.pair.j + "）中心距 = " + fmt(s2.pair.dist) +
  "，半径和 = " + fmt(s2.pair.need));
console.log("  余量（距 − 半径和）  = " + fmt(s2.worst) + "   ← 断言 ≥ 0（collide 没执行就会是 −32 量级）");
console.log("  含 NaN 的节点数      = " + n2 + "   ← 断言 = 0");
console.log("  包围盒（世界单位）   = " + fmt(b2.w, 0) + " × " + fmt(b2.h, 0));
console.log("");

expect(s2.worst >= 0, "场景 2（REP=0）：任意两节点中心距 ≥ 半径和（只可能靠 collide）",
  "最紧余量 " + fmt(s2.worst) + "px");
expect(n2 === 0, "场景 2：没有 NaN / 爆冲", n2 + " 个");
expect(edges2.length === N_EDGES, "场景 2 边数 === " + N_EDGES, "实际 " + edges2.length);
expect(b2.w > 50 && b2.h > 50, "场景 2：节点确实被推开了（不是全缩在原点）",
  fmt(b2.w, 0) + " × " + fmt(b2.h, 0));

if (failures === 0) {
  console.log("ALL PASS（场景 1：" + r1.frames + " 帧 / 余量 " + fmt(s1.worst) + "px；" +
    "场景 2：" + r2.frames + " 帧 / 余量 " + fmt(s2.worst) + "px）");
  process.exit(0);
}
console.log(failures + " 项 FAIL");
process.exit(1);
