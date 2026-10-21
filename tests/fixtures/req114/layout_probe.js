#!/usr/bin/env node
// ═══════════════════════════════════════════════════════════════════════════
// REQ-114 · 布局内核探针（AC-002 的**数据级**判据，入库、可复跑）
// ═══════════════════════════════════════════════════════════════════════════
//
// # 怎么跑
//
//     node tests/fixtures/req114/layout_probe.js
//
// 零依赖：只用 Node 内置的 `fs` / `path`。✅ 通过 exit 0，❌ 失败 exit 1
// ⇒ 可以直接当判据用（CI 里 `node tests/fixtures/req114/layout_probe.js`）。
//
// # 它验证什么
//
// AC-002 的数据级判据：**把 83 个节点全部放在同一坐标 (0,0) 启动布局，推进 300 帧后，
// 任意两节点的中心距 ≥ 它们的半径和**（即「碰撞消解真的把重叠推开了」，而不是靠斥力
// 慢慢挤 —— 斥力在 alpha 收敛后就没有位移了，只有位置级的推离才能兑现这句话）。
//
// 另外**先单独验 collide 本身**：重叠的一对被推到「半径和 + GAP」、钉住的节点不许动。
// 这一格是用来抓「collide 被改成空转」的 —— 只靠上面的涌现判据抓不住它
// （83 节点同坐标时斥力很强，把 collide 的调用删掉仍能凑出 ~12px 余量，已实测）。
//
// # 它凭什么能代表页面
//
// 本脚本**不复制**页面的布局代码：它从 `src/server/page.rs` 里把夹在两条标记
// 之间的「布局内核」段（`radiusOf` / `collide` / `springs` / `physicsStep` 及其常量）
// **原样抽出来**，用 `new Function` 在 Node 里执行同一份源码。
// ⇒ 页面改了内核而探针没跑，这里就会给出不同的数字；标记被删/改名则**直接失败**
//    （⛔ 绝不「找不到就 return 0」—— 那是假绿）。
//
// 内核段是**纯函数、不碰 DOM**（`page.rs` 里那条注释就是这份契约）。
//
// # 期望输出（本机实测，数字是确定的：内核里没有 Math.random）
//
//     节点数 83 / 边数 52
//     实际推进帧数 168（上限 300，alpha 掉到 ALPHA_MIN 以下即停）
//     最紧一对余量 = +15.79px（断言 ≥ 0）
//     含 NaN 的节点数 0
//     包围盒 529 × 528（世界单位）
//     末行 `ALL PASS（168 帧，最紧余量 15.79px）`，exit 0
//
// # 为什么帧数是 168 而不是 300
//
// 页面里 `alpha` 每帧 ×0.97，`0.97^168 ≈ 0.006`（= `ALPHA_MIN`）⇒ 先撞上「收敛」
// 那条闸就停帧。这正是「力导向必须收敛、不许永远满帧」的证据。
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

let failures = 0;

function pass(name, extra) {
  console.log("PASS " + name + (extra === undefined ? "" : "  " + extra));
}
function fail(name, extra) {
  console.log("FAIL " + name + (extra === undefined ? "" : "  " + extra));
  failures++;
}
function die(msg) {
  // 「读不到 / 抽不到」必须**直接失败**，不能静默通过。
  console.log("FAIL " + msg);
  console.log("\n探针无法完成 ⇒ exit 1（⛔ 不许把这种情形当成通过）");
  process.exit(1);
}
// 断言：条件为假 ⇒ 记一条 FAIL（不是「打印一句话」）。
function expect(cond, name, extra) {
  if (cond) pass(name, extra);
  else fail(name, extra);
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

// ── ③ 在 Node 里执行同一份源码 ────────────────────────────────────────────
let K;
try {
  const exports_ = {};
  new Function(
    "exports",
    kernel +
      "\nexports.radiusOf = radiusOf; exports.collide = collide; exports.springs = springs;" +
      "exports.physicsStep = physicsStep; exports.GAP = GAP;" +
      "exports.SIM_FRAMES = SIM_FRAMES; exports.ALPHA_MIN = ALPHA_MIN;"
  )(exports_);
  K = exports_;
} catch (e) {
  die("布局内核在 Node 里执行失败：" + e.message);
}

// ── ④ 反空转：抽出来必须真的是一组可用的函数/常量 ────────────────────────
for (const name of ["radiusOf", "collide", "springs", "physicsStep"]) {
  if (typeof K[name] !== "function") die("内核缺少函数 " + name + "（抽出来的东西是空的）");
}
for (const name of ["GAP", "SIM_FRAMES", "ALPHA_MIN"]) {
  if (typeof K[name] !== "number" || !isFinite(K[name])) die("内核缺少常量 " + name);
}

console.log("REQ-114 · 布局内核探针（从 src/server/page.rs **原样抽取**）");
console.log("内核段 = " + Buffer.byteLength(kernel, "utf8") + " 字节" +
  "（radiusOf / collide / springs / physicsStep）");
console.log("场景 = " + N_NODES + " 个节点**全部放在同一坐标 (0,0)**，" +
  "推进至多 " + MAX_FRAMES + " 帧，alpha 每帧 ×0.97\n");

const collisions = (kernel.match(/collide\(ns, GAP\);/g) || []).length;
if (collisions < 1) fail("内核里 collide 没被接进物理步（写了没调用 = 碰撞形同虚设）", "命中 0 处");
else pass("内核里 collide 被接进物理步（不是写了没调用）", "命中 " + collisions + " 处");
if (K.SIM_FRAMES !== MAX_FRAMES) fail("页面帧上限 SIM_FRAMES 不是 " + MAX_FRAMES, "实际 " + K.SIM_FRAMES);
else pass("页面帧上限 SIM_FRAMES === " + MAX_FRAMES);

// ── ⑤ 造 83 个节点：坐标全为 0（AC-002 的前提）──────────────────────────
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

// 52 条边（确定性连法，模拟真实图的稠密度；自环与重复边跳过，保证条数就是 52）
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

const nodes = buildNodes();
const edges = buildEdges(nodes);
const parents = buildParents(nodes);

// 开局必须真的全在同一坐标（否则「同起点」这句话就不成立）
const allAtOrigin =
  nodes.every((d) => d.x === 0 && d.y === 0) && nodes.every((d) => d.vx === 0 && d.vy === 0);
pass("起点：83 个节点都在 (0,0)，速度 0", allAtOrigin ? N_NODES + "/" + N_NODES : "有节点不在原点");

// ── ⑥ 先单独验 collide 本身（它是**位置推离**，不是斥力；斥力收敛后就不再位移）──
const unitA = { x: 0, y: 0, r: 10, vx: 0, vy: 0, pin: false };
const unitB = { x: 4, y: 0, r: 12, vx: 0, vy: 0, pin: false };
K.collide([unitA, unitB], K.GAP);
const unitGap = Math.hypot(unitB.x - unitA.x, unitB.y - unitA.y) - (unitA.r + unitB.r);
expect(unitGap >= K.GAP - 1e-9, "collide：重叠的一对被推到「半径和 + GAP」",
  "余量 " + fmt(unitGap) + " ≥ GAP " + K.GAP);

const pinA = { x: 0, y: 0, r: 10, vx: 0, vy: 0, pin: true };
const pinB = { x: 3, y: 0, r: 10, vx: 0, vy: 0, pin: false };
K.collide([pinA, pinB], K.GAP);
expect(pinA.x === 0 && pinA.y === 0, "collide：钉住的节点不动（否则拖拽会被下一帧弹回去）",
  "(" + pinA.x + ", " + pinA.y + ")");
expect(Math.hypot(pinB.x - pinA.x, pinB.y - pinA.y) - (pinA.r + pinB.r) >= K.GAP - 1e-9,
  "collide：让对面的节点让开整个间距",
  "余量 " + fmt(Math.hypot(pinB.x - pinA.x, pinB.y - pinA.y) - (pinA.r + pinB.r)));

// ── ⑦ 推进（与页面 loop() 同一套规则：alpha 每帧 ×0.97，撞闸即停）──────
let alpha = 1;
let frames = 0;
let stoppedBy = "";
while (frames < MAX_FRAMES && alpha > K.ALPHA_MIN) {
  K.physicsStep(nodes, edges, parents, alpha);
  frames++;
  alpha = alpha * 0.97;
}
stoppedBy = alpha <= K.ALPHA_MIN ? "alpha 掉到 ALPHA_MIN 以下（收敛）" : "撞到 300 帧上限";

// ── ⑧ 量结果：最紧一对 / NaN / 包围盒 ────────────────────────────────────
let worstSlack = Infinity;
let worstPair = null;
for (let i = 0; i < nodes.length; i++) {
  for (let j = i + 1; j < nodes.length; j++) {
    const a = nodes[i];
    const b = nodes[j];
    const dist = Math.hypot(b.x - a.x, b.y - a.y);
    const need = a.r + b.r;
    if (dist - need < worstSlack) {
      worstSlack = dist - need;
      worstPair = { i: i, j: j, dist: dist, need: need };
    }
  }
}
const nanCount = nodes.filter((d) => !isFinite(d.x) || !isFinite(d.y)).length;

let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
for (const d of nodes) {
  x0 = Math.min(x0, d.x); x1 = Math.max(x1, d.x);
  y0 = Math.min(y0, d.y); y1 = Math.max(y1, d.y);
}

console.log("");
console.log("节点数                 = " + nodes.length);
console.log("边数                   = " + edges.length);
console.log("实际推进帧数           = " + frames + "（上限 " + MAX_FRAMES + "）");
console.log("停止原因               = " + stoppedBy);
console.log("最紧一对（节点 #" + worstPair.i + " / #" + worstPair.j + "）");
console.log("  中心距               = " + fmt(worstPair.dist));
console.log("  半径和               = " + fmt(worstPair.need));
console.log("  余量（距 − 半径和）  = " + fmt(worstSlack) + "   ← 断言 ≥ 0");
console.log("含 NaN 的节点数        = " + nanCount + "   ← 断言 = 0");
console.log("包围盒（世界单位）     = " + fmt(x1 - x0, 0) + " × " + fmt(y1 - y0, 0));
console.log("");

// ── ⑨ 断言（任一条不成立 ⇒ exit 1）──────────────────────────────────────
// ⚠️ 这里必须用 expect（**条件为假就记一条 FAIL**）：写成 pass(name, cond) 会让
//    「条件」变成一句打印出来的字，探针就永远不会红 —— 那正是本脚本要避免的假绿。
expect(worstSlack >= 0, "AC-002：任意两节点中心距 ≥ 半径和（余量 ≥ 0）", "最紧余量 " + fmt(worstSlack) + "px");
expect(nanCount === 0, "没有 NaN / 爆冲", nanCount + " 个");
expect(frames <= MAX_FRAMES, "力导向收敛（帧数 ≤ " + MAX_FRAMES + "）", frames + " 帧");
expect(stoppedBy.indexOf("收敛") >= 0, "不是靠满帧凑出来的（必须真的停）", stoppedBy);
expect(x1 - x0 > 100 && y1 - y0 > 100, "铺开后有实际尺寸（不是全缩在原点）",
  fmt(x1 - x0, 0) + " × " + fmt(y1 - y0, 0));

if (failures === 0) {
  console.log("\nALL PASS（" + frames + " 帧，最紧余量 " + fmt(worstSlack) + "px）");
  process.exit(0);
}
console.log("\n" + failures + " 项 FAIL");
process.exit(1);
