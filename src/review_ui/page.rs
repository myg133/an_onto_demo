//! REQ-111 · 审核界面页面 —— **独立 HTML**（R4）、零外部依赖、壳内零数据。
//!
//! # 为什么与 `server/page.rs` 分开
//!
//! R4 的裁定：复用**样式与组件做法**，但**不复用那个文件** ——
//! 「客户看结果」与「FDE 做裁决」是两套语义，混在一个 HTML 里，
//! 下一个改导航栏的人会同时改到两边。两页各自自包含，代价是几段 CSS 重复，
//! 换来的是「改审核页不会碰客户页」这条结构边界。
//!
//! # 壳里零数据
//!
//! 这个常量里**没有**项目名 / 提案名 / 条目 / 决策 —— 全部在拿到口令后走
//! `GET /api/review`。于是 `/` 可以免口令（浏览器导航带不了 `Authorization`），
//! 而「能探测到有个服务在」是唯一代价（与 REQ-110 的 ADR-004 同一条取舍）。
//!
//! # 四条交互纪律（写进页面，不只写在文档里）
//!
//! 1. **逐条签**：每条 × 每轨各两个按钮，点了立刻显示服务端回执或**具体原因**；
//! 2. **批量先预览**：批量只按「档位/建议 + 未签」算候选，先 `GET /api/preview` 拿
//!    条数与白名单，弹窗显示「将接受 N 条」并列出条目号，**确认后**才逐条提交；
//! 3. **没有一键全库**：批量面板不接受「全部条目」，也不含「全选」按钮；
//!    真正的提交是一串单条 `POST /api/decide`（服务端没有批量写接口）。
//! 4. ⭐ REQ-112 · **冲突单独成组、排最前**：建议为 `conflict` 的条目渲染在
//!    独立的 section 里，**不与接受/拒绝混排** —— 它们是唯一一类「程序不知道该
//!    听谁的」条目，混进普通列表就会被当普通建议扫过去（需求风险 1）。
//!
//! # REQ-112 · 页面里**不重算**建议
//!
//! 六种 `kind` 与依据文案全部来自 `GET /api/review` 的 `suggestion` 字段；
//! 分组计数来自同一响应的 `suggestion_counts`。页面只做三件事：映射成中文标签、
//! 按 `kind` 分组、把 `kind.enters_batch` 的结果（由服务端白名单体现）拿去弹窗。
//! 档位/权重常量一个都不在 HTML 里 —— 那是 `domain/confidence.rs` 的事（R6 / AC-009）。

/// 审核页面本体。**自包含**：CSS / JS 全内联，不引任何外部资源。
pub const REVIEW_HTML: &str = r##"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>提案审核</title>
<style>
:root{--bg:#fbfbfd;--fg:#1d1d1f;--mut:#6e6e73;--line:#d2d2d7;--card:#fff;--acc:#0a6cff;--ok:#0a8f3c;--bad:#c2261c;--warn:#b26a00}
*{box-sizing:border-box}
body{margin:0;font:14px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Hiragino Sans GB","Microsoft YaHei",sans-serif;background:var(--bg);color:var(--fg)}
header{position:sticky;top:0;z-index:5;background:var(--card);border-bottom:1px solid var(--line);padding:10px 16px;display:flex;gap:10px;align-items:center;flex-wrap:wrap}
h1{font-size:15px;margin:0;font-weight:600;white-space:nowrap}
#scope{color:var(--mut);font-size:13px}
input,select{font:inherit;padding:5px 9px;border:1px solid var(--line);border-radius:6px;background:var(--card)}
input[type=password]{width:180px}
button{font:inherit;padding:4px 11px;border:1px solid var(--line);border-radius:6px;background:var(--card);cursor:pointer;white-space:nowrap}
button:hover{border-color:var(--acc);color:var(--acc)}
button:disabled{opacity:.45;cursor:default}
button.ok{border-color:var(--ok);color:var(--ok)}
button.bad{border-color:var(--bad);color:var(--bad)}
#msg{font-size:13px;flex:1;min-width:200px}
#msg.bad{color:var(--bad);font-weight:600}
#msg.ok{color:var(--ok)}
#progress{padding:8px 16px;border-bottom:1px solid var(--line);background:var(--card);display:flex;gap:18px;align-items:center;flex-wrap:wrap;font-size:13px;color:var(--mut)}
#bar{flex:1;min-width:160px;height:8px;background:#e8e8ed;border-radius:4px;overflow:hidden}
#bar>i{display:block;height:100%;width:0;background:var(--acc);transition:width .2s}
#controls{padding:10px 16px;display:flex;gap:10px;align-items:center;flex-wrap:wrap;border-bottom:1px solid var(--line);background:#f4f4f7}
#controls .sep{width:1px;height:22px;background:var(--line)}
#controls .note{color:var(--mut);font-size:12px}
main{padding:12px 16px 60px;max-width:1180px;margin:0 auto}
article{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:10px 12px;margin-bottom:10px}
article.vetoed{border-color:#e0a39e;background:#fff7f6}
article.passed{border-color:#a9d4b8;background:#f7fffa}
.hd{display:flex;gap:10px;align-items:baseline;flex-wrap:wrap}
.idx{font-variant-numeric:tabular-nums;color:var(--mut);font-size:12px}
.target{font-weight:600}
.kind{font-size:12px;color:var(--mut);border:1px solid var(--line);border-radius:999px;padding:0 8px}
.band{font-size:12px;padding:0 7px;border-radius:999px;border:1px solid var(--line);color:var(--mut)}
.band.high{border-color:#a9d4b8;color:var(--ok)}
.band.mid{border-color:#e8cf9a;color:var(--warn)}
.band.low{border-color:#e0a39e;color:var(--bad)}
/* REQ-112 · 建议标签（六种 kind，颜色只做「要不要人看」的提示，不是结论） */
.sugg{font-size:12px;padding:0 8px;border-radius:999px;border:1px solid var(--line);font-weight:600}
.sugg.accept{color:var(--ok);border-color:#a9d4b8;background:#f2fbf5}
.sugg.lean_accept{color:#2f6f3f;border-color:#bcdcc6}
.sugg.review{color:var(--warn);border-color:#e8cf9a;background:#fffdf4}
.sugg.reject{color:var(--bad);border-color:#e0a39e;background:#fff7f6}
.sugg.conflict{color:#fff;background:var(--bad);border-color:var(--bad)}
.sugg.none{color:var(--mut)}
.suggwhy{margin-top:5px;font-size:12px;color:var(--mut)}
.suggwhy b{color:var(--fg);font-weight:600}
h2.grp{font-size:13px;margin:14px 0 8px;color:var(--mut);font-weight:600}
h2.grp.conflict{color:var(--bad)}
h2.grp .cnt{font-weight:400}
#suggbar{padding:6px 16px;border-bottom:1px solid var(--line);background:var(--card);display:flex;gap:14px;align-items:center;flex-wrap:wrap;font-size:13px;color:var(--mut)}
#suggbar b{color:var(--fg);font-variant-numeric:tabular-nums}
#suggbar .conflict b{color:var(--bad)}
/* REQ-113 · 被排除的表（只读区块：没有按钮、没有输入框） */
#excluded,#flagged{margin:12px 16px 0;background:var(--card);border:1px solid var(--line);border-radius:10px;padding:10px 12px}
#excluded .x-head,#flagged .x-head{font-size:13px;font-weight:600;color:var(--fg)}
#excluded .x-head b,#flagged .x-head b{font-variant-numeric:tabular-nums}
#excluded .x-note,#flagged .x-note{font-weight:400;color:var(--mut);font-size:12px}
#excluded .x-meta{margin-top:4px;font-size:12px;color:var(--mut);word-break:break-all}
#excluded .x-meta.bad{color:var(--bad);font-weight:600}
#excluded table,#flagged table{width:100%;border-collapse:collapse;margin-top:8px;font-size:13px}
#excluded th,#excluded td,#flagged th,#flagged td{text-align:left;padding:4px 8px;border-bottom:1px solid var(--line);vertical-align:top}
#excluded th,#flagged th{font-weight:600;font-size:12px;color:var(--mut)}
#excluded td.mono,#flagged td.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:12px}
#excluded .x-ev{margin-top:3px;font-size:12px;color:var(--mut)}
#excluded .x-stale{margin-top:8px;font-size:12px;color:var(--warn)}
#excluded[hidden],#flagged[hidden]{display:none}
.tracks{display:flex;gap:22px;flex-wrap:wrap;margin-top:8px}
.track{display:flex;gap:7px;align-items:center;font-size:13px}
.track .name{color:var(--mut)}
.st{font-size:12px;border-radius:5px;padding:1px 7px;border:1px solid var(--line)}
.st.accepted{color:var(--ok);border-color:#a9d4b8}
.st.rejected{color:var(--bad);border-color:#e0a39e}
.st.pending{color:var(--mut)}
.why{margin-top:8px;font-size:13px;color:var(--bad)}
.why.grey{color:var(--mut)}
.detail{margin-top:6px;font-size:13px;color:var(--mut)}
.detail b{color:var(--fg);font-weight:600}
pre{margin:6px 0 0;padding:8px;background:#f4f4f7;border-radius:6px;font-size:12px;overflow:auto;max-height:260px}
details{margin-top:8px}
summary{cursor:pointer;color:var(--acc);font-size:13px}
#modal{position:fixed;inset:0;background:rgba(0,0,0,.35);display:flex;align-items:center;justify-content:center;padding:20px;z-index:20}
#modal[hidden]{display:none}
#box{background:var(--card);border-radius:12px;max-width:620px;width:100%;max-height:80vh;overflow:auto;padding:18px 20px}
#box h2{margin:0 0 6px;font-size:16px}
#box p{margin:6px 0;color:var(--mut);font-size:13px}
#box .acts{display:flex;gap:10px;justify-content:flex-end;margin-top:14px}
#ids{font-size:12px;color:var(--mut);word-break:break-all;background:#f4f4f7;border-radius:6px;padding:8px;max-height:180px;overflow:auto}
.empty{color:var(--mut);text-align:center;padding:40px 0}
</style>
</head>
<body>
<header>
  <h1>提案审核</h1>
  <span id="scope">未载入</span>
  <input id="token" type="password" placeholder="访问口令" autocomplete="off">
  <button id="load">载入口令</button>
  <span id="msg">口令只留在本页内存里，不进地址栏、不进浏览器存储。</span>
</header>

<div id="progress">
  <span>已签 <b id="p-signed">0</b> / <span id="p-total">0</span></span>
  <span>待签 <b id="p-pending">0</b></span>
  <span>两轨通过 <b id="p-passed">0</b></span>
  <span>被否 <b id="p-vetoed">0</b></span>
  <span id="bar"><i></i></span>
</div>

<div id="suggbar">
  <span>建议（后端计数）：
    冲突 <b id="s-conflict">0</b> ·
    拒绝 <b id="s-reject">0</b> ·
    人工核对 <b id="s-review">0</b> ·
    倾向接受 <b id="s-lean_accept">0</b> ·
    接受 <b id="s-accept">0</b> ·
    无建议 <b id="s-none">0</b>
  </span>
  <span>冲突与无建议<b>永不参与</b>「按建议接受」。</span>
</div>

<!-- REQ-113 · 被排除的表 —— **只读**区块（表名 / 类别 / 理由 / 决策人 / 置信度）。
     这里一个按钮都没有，也没有输入框：改排除要改 exclusions.json 重跑（AC-006）。
     数据全部来自 GET /api/review 的 `exclusions` 块（服务端读提案 inputs），
     页面**不重算**「哪些表被排除」—— 它连 schema 都没有。 -->
<section id="excluded" hidden>
  <div class="x-head">被排除 <b id="x-count">0</b> 张
    <span class="x-note">（只读 · 改排除要改 exclusions.json 重跑，页面上不能编辑）</span>
  </div>
  <div class="x-meta" id="x-meta"></div>
  <table>
    <thead><tr><th style="width:24%">表</th><th style="width:15%">类别</th><th>理由</th><th style="width:15%">决策人</th><th style="width:9%">置信度</th></tr></thead>
    <tbody id="x-body"></tbody>
  </table>
  <div class="x-stale" id="x-stale"></div>
</section>

<!-- REQ-113 · **保留但存疑** —— 与上面那栏**语义相反**，⛔ 不许合并：
     上面是「已决定排除（不建实体）」，这里是「照常建、但请人看一眼」。
     两块分开显示，是因为一个是结论、一个是问题。 -->
<section id="flagged" hidden>
  <div class="x-head">保留但存疑 <b id="f-count">0</b> 张
    <span class="x-note">（**没有被排除** · 已照常进本体 · 待裁决）</span>
  </div>
  <table>
    <thead><tr><th style="width:24%">表</th><th>为什么拿不准</th><th style="width:34%">担心什么 / 建议谁看</th></tr></thead>
    <tbody id="f-body"></tbody>
  </table>
</section>

<section id="controls">
  <label>状态
    <select id="f-status">
      <option value="all">全部</option>
      <option value="unsigned" selected>未签</option>
      <option value="signed">已签</option>
      <option value="vetoed">被否</option>
    </select>
  </label>
  <label>置信度
    <select id="f-band">
      <option value="">不限</option>
      <option value="0.9">≥ 0.90</option>
      <option value="0.75">≥ 0.75</option>
      <option value="0.6">≥ 0.60</option>
      <option value="none">无分数</option>
    </select>
  </label>
  <label>搜索 <input id="f-q" placeholder="条目号 / 名称" style="width:140px"></label>
  <label>按建议
    <select id="f-sugg">
      <option value="">不限</option>
      <option value="conflict">冲突（需人工判）</option>
      <option value="accept">接受</option>
      <option value="lean_accept">倾向接受</option>
      <option value="review">人工核对</option>
      <option value="reject">拒绝</option>
      <option value="none">无建议</option>
    </select>
  </label>
  <span class="sep"></span>
  <label>批量轨
    <select id="b-track"><option value="biz">业务轨</option><option value="model">建模轨</option></select>
  </label>
  <label>批量口径
    <select id="b-mode"><option value="suggestion">按建议</option><option value="band">按置信度档位</option></select>
  </label>
  <label>批量档位
    <select id="b-band"><option value="0.6">≥ 0.60</option><option value="0.75">≥ 0.75</option><option value="0.9">≥ 0.90</option></select>
  </label>
  <button id="b-preview">预览批量接受…</button>
  <span class="note">批量只作用于**未签**条目，且必须先看条数；「按建议」只含接受/倾向接受，**冲突与无建议不参与**；没有「一键全库」。</span>
</section>

<main id="list"><div class="empty">填入口令并点「载入口令」。</div></main>

<div id="modal" hidden>
  <div id="box">
    <h2 id="m-title">预览</h2>
    <p id="m-sub"></p>
    <div id="ids"></div>
    <div class="acts">
      <button id="m-cancel">取消</button>
      <button id="m-go" class="ok" disabled>确认</button>
    </div>
  </div>
</div>

<script>
"use strict";
var S = { token: "", data: null, filter: "unsigned", band: "", sugg: "", query: "", batch: null, busy: false };

function $(id) { return document.getElementById(id); }
function msg(text, cls) { var el = $("msg"); el.textContent = text; el.className = cls || ""; }
function el(tag, cls, text) { var n = document.createElement(tag); if (cls) n.className = cls; if (text !== undefined && text !== null) n.textContent = text; return n; }

function api(path, opts) {
  opts = opts || {};
  opts.headers = opts.headers || {};
  opts.headers["Authorization"] = "Bearer " + S.token;
  if (opts.body) opts.headers["Content-Type"] = "application/json";
  return fetch(path, opts).then(function (r) {
    return r.text().then(function (t) {
      var body = null;
      try { body = t ? JSON.parse(t) : null; } catch (e) { body = null; }
      if (!r.ok) {
        var why = body && body.error ? body.error : ("HTTP " + r.status + (t ? ("：" + t.slice(0, 200)) : ""));
        throw new Error(why);
      }
      return body;
    });
  });
}

function load() {
  S.token = $("token").value.trim();
  if (!S.token) { msg("先填访问口令。", "bad"); return; }
  api("api/review").then(function (d) {
    S.data = d; S.filter = $("f-status").value; S.band = $("f-band").value; S.query = "";
    $("scope").textContent = d.project + " · 提案 " + d.proposal + " · 基于 v" + d.base_version;
    msg("已载入 " + d.items.length + " 条。", "ok");
    render();
  }).catch(function (e) { msg("载入失败：" + e.message, "bad"); });
}

function score(it) {
  if (it.confidence && typeof it.confidence.score === "number") return it.confidence.score;
  return null;
}
function bandName(it) {
  var c = it.confidence;
  if (!c) return "无分数";
  return (c.band || "") + (typeof c.score === "number" ? " " + c.score.toFixed(2) : "");
}
function isUnsigned(it, track) { return (track === "biz" ? it.biz : it.model) === "pending"; }
function bothSigned(it) { return it.biz !== "pending" && it.model !== "pending"; }
function isVetoed(it) { return it.biz === "rejected" || it.model === "rejected"; }

// REQ-112 · 建议只**读后端发的** `suggestion` 字段 —— 页面里不重新判规则。
// 六种 kind 与依据文案都来自 GET /api/review；这里只做显示映射。
var SUGG_LABEL = { accept: "接受", lean_accept: "倾向接受", review: "人工核对", reject: "拒绝", conflict: "冲突", none: "无建议" };
function suggOf(it) { return (it && it.suggestion) ? it.suggestion : { kind: "none", reason: "" }; }
function suggLabel(k) { return SUGG_LABEL[k] || k; }

function visible(it) {
  if (S.filter === "unsigned" && !(isUnsigned(it, "biz") || isUnsigned(it, "model"))) return false;
  if (S.filter === "signed" && !bothSigned(it)) return false;
  if (S.filter === "vetoed" && !isVetoed(it)) return false;
  // 「按建议筛选」：值直接是后端 kind，页面不猜。
  if (S.sugg && suggOf(it).kind !== S.sugg) return false;
  if (S.band) {
    var sc = score(it);
    if (S.band === "none") { if (sc !== null) return false; }
    else if (sc === null || sc + 1e-9 < parseFloat(S.band)) return false;
  }
  if (S.query) {
    var q = S.query.toLowerCase();
    if (String(it.item) !== q && it.target.toLowerCase().indexOf(q) < 0) return false;
  }
  return true;
}

function statusBadge(v) {
  var map = { accepted: "已接受", rejected: "已拒绝", pending: "未签" };
  return el("span", "st " + v, map[v] || v);
}

function trackRow(it, track, label) {
  var wrap = el("div", "track");
  wrap.appendChild(el("span", "name", label + "："));
  var v = track === "biz" ? it.biz : it.model;
  wrap.appendChild(statusBadge(v));
  var via = it.via && it.via[track] ? (" · 最后由 " + (it.via[track] === "ui" ? "页面" : "CLI") + " 于 " + (it.decided_at[track] || "")) : "";
  if (via) wrap.appendChild(el("span", "name", via));
  var a = el("button", "ok", "接受");
  a.disabled = S.busy;
  a.onclick = function () { sign(it, track, "accept"); };
  var r = el("button", "bad", "拒绝");
  r.disabled = S.busy;
  r.onclick = function () { sign(it, track, "reject"); };
  wrap.appendChild(a); wrap.appendChild(r);
  return wrap;
}

function card(it) {
  var sg = suggOf(it);
  var card = el("article", isVetoed(it) ? "vetoed" : (it.state === "passed" ? "passed" : ""));
  if (sg.kind === "conflict") card.className += " conflict";
  var hd = el("div", "hd");
  hd.appendChild(el("span", "idx", "#" + it.item));
  hd.appendChild(el("span", "target", it.target));
  hd.appendChild(el("span", "kind", it.kind));
  // ⭐ 建议列：标签带 `title`（悬停看整句），依据在下面单独一行 ——
  // 「人把建议当结论、不再看依据」是需求写明的风险 1，所以依据不藏进 tooltip 里。
  var sb = el("span", "sugg " + sg.kind, "建议：" + suggLabel(sg.kind));
  sb.title = sg.reason || "";
  hd.appendChild(sb);
  var sc = score(it), b = el("span", "band", bandName(it));
  if (sc !== null) b.className += sc >= 0.9 ? " high" : (sc >= 0.75 ? " mid" : " low");
  hd.appendChild(b);
  card.appendChild(hd);
  card.appendChild(el("div", "suggwhy", "依据：" + (sg.reason || "（无）")));

  var tracks = el("div", "tracks");
  tracks.appendChild(trackRow(it, "biz", "业务轨"));
  tracks.appendChild(trackRow(it, "model", "建模轨"));
  card.appendChild(tracks);

  if (it.reason) card.appendChild(el("div", "why", "阻断原因：" + it.reason));
  var ai = it.confidence && it.confidence.ai && it.confidence.ai.rationale;
  if (ai) card.appendChild(el("div", "detail", "AI 理由：" + ai));

  if (it.evidence && it.evidence.length) {
    var ev = it.evidence.map(function (e) {
      if (typeof e === "string") return e;
      return [e.claim, e.table ? ("表 " + e.table) : "", e.column ? ("列 " + e.column) : "", e.inference ? ("推断 " + e.inference) : ""]
        .filter(Boolean).join(" · ");
    }).join("；");
    card.appendChild(el("div", "detail", "证据：" + ev));
  }

  var d = el("details");
  d.appendChild(el("summary", null, "候选定义（建模内容）"));
  d.appendChild(el("pre", null, JSON.stringify(it.payload, null, 2)));
  card.appendChild(d);
  return card;
}

function groupHead(title, cls, n) {
  var h = el("h2", "grp" + (cls ? " " + cls : ""), title);
  h.appendChild(el("span", "cnt", " · " + n + " 条"));
  return h;
}

// REQ-113 · 「被排除的表」与「保留但存疑」—— 两块**分开**渲染。
// 三个数（count / flagged_count / tables / flagged / stale）与 sha256 全部来自
// 后端那一份记录，页面既不重算也不缓存：改排除的唯一办法是改文件重跑（AC-006）。
function renderExcluded() {
  var x = (S.data && S.data.exclusions) ? S.data.exclusions : { present: false };
  var sec = $("excluded"), fsec = $("flagged");
  sec.hidden = false;
  $("x-count").textContent = x.present ? x.count : 0;
  var body = $("x-body");
  body.textContent = "";
  var fbody = $("f-body");
  fbody.textContent = "";
  var meta = $("x-meta");
  meta.className = "x-meta";
  var stale = $("x-stale");
  stale.textContent = "";
  if (!x.present) {
    // 三种「没有记录」都要说出来（不知道 / 没排除 / 记录坏了），
    // 合并成一句「无」会让「被改坏的溯源」看起来像「本来就没有」。
    meta.textContent = x.reason || "没有排除记录。";
    if (x.broken) meta.className = "x-meta bad";
    fsec.hidden = true;
    return;
  }
  // 「谁判的」与「谁签的」分开展示：AI 先判一轮时两者不是同一个人。
  meta.textContent = "文件 " + x.file + " · version " + x.version +
    " · sha256 " + String(x.sha256).slice(0, 12) + "…" +
    " · 判的人 " + x.declared_by +
    (x.reviewed_by ? (" · 签的人 " + x.reviewed_by) : "") +
    " · 规则：" + x.rule;
  (x.tables || []).forEach(function (e) {
    var tr = document.createElement("tr");
    tr.appendChild(el("td", "mono", e.table));
    tr.appendChild(el("td", null, e.category_label || e.category));
    // 理由 + 判定依据（依据若给就显示在下面 —— 评审要推翻一条排除，靠的是依据）
    var whyTd = document.createElement("td");
    whyTd.appendChild(el("div", null, e.reason));
    if (e.evidence) whyTd.appendChild(el("div", "x-ev", "依据：" + e.evidence));
    tr.appendChild(whyTd);
    tr.appendChild(el("td", null, e.decided_by));
    // 置信度是**可选**的：`null` 显示「—」而不是 0.00 ——
    // 「没有这个量」（人工判断）与「AI 完全不确定」是两件事。
    tr.appendChild(el("td", null, (typeof e.confidence === "number") ? e.confidence.toFixed(2) : "—"));
    body.appendChild(tr);
  });
  // ⛔ 待裁决：**另一块**，且明确写「没有被排除、已进本体」。
  var flagged = x.flagged || [];
  $("f-count").textContent = flagged.length;
  fsec.hidden = flagged.length === 0;
  flagged.forEach(function (f) {
    var tr = document.createElement("tr");
    tr.appendChild(el("td", "mono", f.table));
    tr.appendChild(el("td", null, f.why));
    tr.appendChild(el("td", null, f.concern || "—"));
    fbody.appendChild(tr);
  });
  if (x.stale && x.stale.length) {
    stale.textContent = "⚠ 陈旧排除 " + x.stale.length + " 条（文件里点名、但这份 schema 里没有）：" + x.stale.join("、");
  }
}

function render() {
  if (!S.data) return;
  var items = S.data.items;
  var p = S.data.progress;
  $("p-total").textContent = p.total;
  $("p-signed").textContent = p.signed;
  $("p-pending").textContent = p.pending;
  $("p-passed").textContent = p.passed;
  $("p-vetoed").textContent = p.vetoed;
  var pct = p.total ? Math.round((p.signed / p.total) * 100) : 0;
  $("bar").firstElementChild.style.width = pct + "%";

  // 分组计数**用后端发的那一份**（AC-008：分组计数与后端一致）。
  var counts = S.data.suggestion_counts || {};
  ["conflict", "reject", "review", "lean_accept", "accept", "none"].forEach(function (k) {
    var box = $("s-" + k);
    if (box) box.textContent = counts[k] === undefined ? 0 : counts[k];
  });

  var list = $("list");
  list.textContent = "";

  // REQ-113 · 排除区块跟着同一次 load 一起渲染（数据也来自同一个响应）。
  renderExcluded();

  // ⭐ REQ-112 · **冲突单独成组、排最前、不与接受/拒绝混排**（AC-008）。
  // 其余条目按原顺序；冲突卡片不会出现在「其余」里。
  var conflict = [], rest = [];
  items.forEach(function (it) {
    if (!visible(it)) return;
    if (suggOf(it).kind === "conflict") conflict.push(it);
    else rest.push(it);
  });

  if (conflict.length) {
    list.appendChild(groupHead("⚠ 冲突 —— 证据支持但 AI 否定，需人工判", "conflict", conflict.length));
    conflict.forEach(function (it) { list.appendChild(card(it)); });
  }
  if (rest.length) {
    list.appendChild(groupHead("其余条目", "", rest.length));
    rest.forEach(function (it) { list.appendChild(card(it)); });
  }
  if (!conflict.length && !rest.length) {
    list.appendChild(el("div", "empty", "当前筛选下没有条目。"));
  }
}

function mergeReply(reply) {
  var it = S.data.items.filter(function (x) { return x.item === reply.item; })[0];
  if (it) {
    it.biz = reply.biz; it.model = reply.model; it.state = reply.state;
    it.decided_at = reply.decided_at; it.via = reply.via;
  }
  S.data.progress = reply.progress;
}

function sign(it, track, decision) {
  if (S.busy) return;
  S.busy = true; render();
  api("api/decide", { method: "POST", body: JSON.stringify({ item: it.item, track: track, decision: decision }) })
    .then(function (reply) {
      mergeReply(reply);
      msg("#" + reply.item + " 的" + (track === "biz" ? "业务轨" : "建模轨") + " → " + (decision === "accept" ? "已接受" : "已拒绝") + "（入口 ui）", "ok");
      S.busy = false; render();
    })
    .catch(function (e) {
      // AC-007：把服务端的**具体原因**原样显示 —— 不写成「失败」两个字。
      msg("#" + it.item + " 签不下去：" + e.message, "bad");
      S.busy = false; render();
    });
}

function preview() {
  if (!S.token || !S.data) { msg("先载入口令。", "bad"); return; }
  var track = $("b-track").value, mode = $("b-mode").value, band = $("b-band").value;
  // ⚠️ 两种口径**分开请求**：混在一起时「将接受 N 条」里的 N 就没有唯一含义，
  // 服务端也会拒绝（by=suggestion 与 min_confidence 不能同时给）。
  var url = "api/preview?track=" + encodeURIComponent(track) + "&scope=unsigned";
  url += (mode === "suggestion") ? "&by=suggestion" : ("&min_confidence=" + encodeURIComponent(band));
  api(url)
    .then(function (p) {
      S.batch = { track: track, mode: mode, items: p.items, count: p.count };
      var scope = (track === "biz" ? "业务轨" : "建模轨") + " · 仅未签";
      if (mode === "suggestion") {
        $("m-title").textContent = "按建议接受 " + p.count + " 条（" + scope + " · 建议=接受/倾向接受）";
        $("m-sub").textContent = "总条目 " + p.total + "；已签 " + p.excluded.decided + " 条；" +
          "冲突 " + p.excluded.conflict + " 条、人工核对 " + p.excluded.review + " 条、拒绝 " +
          p.excluded.reject + " 条、无建议 " + p.excluded.none + " 条 —— **这些不参与**。确认后逐条提交（每条一次 POST）。";
      } else {
        $("m-title").textContent = "将接受 " + p.count + " 条（" + scope + " · 置信度 ≥ " + band + "）";
        // N-2：按档位也有白名单（与「按建议」同一个判据）—— 冲突 / 人工核对 / 拒绝
        // 即使分数过线也**不参与**，弹窗必须把它们数出来，否则「将接受 N 条」会让人
        // 以为剩下的也被一起接受了。
        $("m-sub").textContent = "总条目 " + p.total + "；已签 " + p.excluded.decided + " 条；低于档位 " +
          p.excluded.below_band + " 条；无分数 " + p.excluded.no_confidence + " 条；" +
          "冲突 " + p.excluded.conflict + " 条、人工核对 " + p.excluded.review + " 条、拒绝 " +
          p.excluded.reject + " 条 —— **这些不参与**。确认后逐条提交（每条一次 POST）。";
      }
      $("ids").textContent = p.items.length ? ("条目号：" + p.items.join(", ")) : "没有符合条件的条目。";
      $("m-go").textContent = "确认接受 " + p.count + " 条";
      $("m-go").disabled = p.count === 0;
      $("modal").hidden = false;
    })
    .catch(function (e) { msg("预览失败：" + e.message, "bad"); });
}

function runBatch() {
  var b = S.batch;
  if (!b || !b.items.length) return;
  $("modal").hidden = true;
  S.busy = true; render();
  var done = 0, failed = null;
  function step() {
    if (done >= b.items.length || failed) {
      S.busy = false; render();
      if (failed) msg("批量在中途停下：" + failed, "bad");
      else msg("批量完成：接受 " + b.items.length + " 条（" + (b.track === "biz" ? "业务轨" : "建模轨") + "）。", "ok");
      S.batch = null;
      return;
    }
    var item = b.items[done++];
    api("api/decide", { method: "POST", body: JSON.stringify({ item: item, track: b.track, decision: "accept" }) })
      .then(function (reply) { mergeReply(reply); render(); msg("批量进行中 " + done + "/" + b.items.length, ""); step(); })
      .catch(function (e) { failed = "#" + item + "：" + e.message; step(); });
  }
  step();
}

$("load").onclick = load;
$("token").addEventListener("keydown", function (e) { if (e.code === "Enter") load(); });
$("f-status").onchange = function () { S.filter = this.value; render(); };
$("f-band").onchange = function () { S.band = this.value; render(); };
$("f-sugg").onchange = function () { S.sugg = this.value; render(); };
$("f-q").oninput = function () { S.query = this.value.trim(); render(); };
// 口径切到「按建议」时，「批量档位」不再参与计算 —— 置灰而不是藏起来，
// 免得人以为档位还在生效。
$("b-mode").onchange = function () { $("b-band").disabled = (this.value === "suggestion"); };
$("b-band").disabled = true;
$("b-preview").onclick = preview;
$("m-cancel").onclick = function () { $("modal").hidden = true; S.batch = null; };
$("m-go").onclick = runBatch;
</script>
</body>
</html>
"##;
