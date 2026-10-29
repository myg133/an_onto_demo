//! ⭐ REQ-162 · **本地评审台的一屏两栏页面壳**（自包含单文件 · 零外部资源 · 零数据）。
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │ <工作目录> · 草稿 135 条 · 已签 42 · 定版 v1 · 本机（无口令） │
//! ├────────────────────────────────┬─────────────────────────────┤
//! │ 评审（核心）                    │ 导出                         │
//! │  云服务器 → 计算资源  [✓][✗]    │ 定版：v1（14:50）· 135 条     │
//! │  …（业务轨 / 模型轨两列）        │ [导出构建包]                 │
//! │ [全选接受] [全选拒绝]           │ → xxx-v1.ontokbundle         │
//! └────────────────────────────────┴─────────────────────────────┘
//! ```
//!
//! # 页面只做四件事
//!
//! 读草稿 · 记裁决 · 定版 · 导出包。⛔ 没有登录、没有项目选择、没有上传 / DSN /
//! 入库 / 激活 / key / 审计 —— 那些是 Studio 的事（ADR-013）。
//!
//! # 三条硬纪律（都有判据钉着）
//!
//! 1. **壳内零数据**：`GET /` 是匿名可达的，正文是一个**常量** —— 项目名 / 条数 /
//!    版本号全部由 `/api/*` 在浏览器里填（`tests/test_req162_console.rs`）。
//! 2. **零外部资源**：没有 `<script src=` / `<link` / 字库 / 图标 / CDN，
//!    CSP 只放行同源。页面在断网机器上必须长得一模一样。
//! 3. **不留浏览器状态**：⛔ 浏览器侧不写任何持久化 / 会话存储：裁决与定版
//!    都在工作目录的文件里，刷新页面从服务端重读。用户数据只走 `textContent`。
//!
//! # 为什么不用前端框架
//!
//! 一屏两栏、一个列表、四个按钮 —— 手写两百行 JS 就够，而引一个框架意味着
//! 构建产物 / 版本 / 许可 / 体积四件新事。REQ-125 起的纪律是「一个文件、零依赖」。

/// 页面壳（**字节常量**；`GET /` 原样送出，服务端一个字节都不注入）。
pub const CONSOLE_HTML: &str = r####"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>本地评审台 · 草稿 → 定版 → 导出包</title>
<style>
:root{
  color-scheme:dark;
  --bg:#070a10;--bg2:#0b0f17;--card:#111721;--head:#0d131c;
  --line:rgba(255,255,255,.09);--line2:rgba(255,255,255,.05);
  --fg:#e8edf6;--mut:#9aa7bd;--mut2:#8b97ad;
  --acc:#6ec8ff;--ok:#5fd3a3;--warn:#e6b455;--bad:#ef7a8a;
  --mono:ui-monospace,"Cascadia Mono",Consolas,monospace;
}
*{box-sizing:border-box}
html,body{height:100%}
body{margin:0;background:radial-gradient(1200px 600px at 20% -10%,#101a2b 0%,var(--bg) 60%);
  color:var(--fg);font:14px/1.55 "Segoe UI",system-ui,-apple-system,"Microsoft YaHei",sans-serif}
button{font:inherit;color:inherit;background:transparent;border:0;cursor:pointer}
a{color:var(--acc);text-decoration:none}
a:hover{text-decoration:underline}

header{display:flex;align-items:center;gap:14px;flex-wrap:wrap;
  padding:12px 20px;border-bottom:1px solid var(--line);background:rgba(9,13,20,.86)}
header h1{margin:0;font-size:15px;font-weight:650;letter-spacing:.4px}
header .meta{color:var(--mut);font-size:12.5px;display:flex;gap:10px;flex-wrap:wrap}
header .meta b{color:var(--fg);font-weight:600}
header .badge{margin-left:auto;font-size:12px;color:var(--ok);border:1px solid rgba(95,211,163,.35);
  background:rgba(95,211,163,.1);border-radius:999px;padding:2px 10px;white-space:nowrap}

#banner{display:none;margin:10px 20px 0;padding:9px 12px;border-radius:8px;font-size:13px;
  border:1px solid rgba(230,180,85,.4);background:rgba(230,180,85,.1);color:#f2d79b}
#banner.on{display:block}

#error{display:none;margin:10px 20px 0;padding:10px 12px;border-radius:8px;font-size:13px;
  border:1px solid rgba(239,122,138,.45);background:rgba(239,122,138,.1)}
#error.on{display:block}
#error .t{color:#ffb3be;font-weight:600;margin-bottom:2px}
#error .m{color:#ffd9de;white-space:pre-wrap}
#error .h{color:var(--mut);margin-top:4px;white-space:pre-wrap}

main{display:grid;grid-template-columns:minmax(0,1fr) 360px;gap:16px;padding:16px 20px 24px;
  align-items:start}
@media (max-width:980px){main{grid-template-columns:minmax(0,1fr)}}

section{background:var(--card);border:1px solid var(--line);border-radius:12px;overflow:hidden}
section>h2{margin:0;padding:11px 14px;font-size:13px;font-weight:650;letter-spacing:.4px;
  background:var(--head);border-bottom:1px solid var(--line);display:flex;gap:10px;align-items:center}
section>h2 .hint{margin-left:auto;color:var(--mut2);font-weight:400;font-size:12px}

#review{display:flex;flex-direction:column;max-height:calc(100vh - 150px)}
#review .bar{display:flex;align-items:center;gap:12px;padding:10px 14px;border-bottom:1px solid var(--line2)}
#track{flex:1;height:6px;border-radius:999px;background:rgba(255,255,255,.07);overflow:hidden}
#trackFill{height:100%;width:0;background:linear-gradient(90deg,#3aa4e8,#5fd3a3);transition:width .18s}
#progressText{font-size:12px;color:var(--mut);white-space:nowrap}
#review .tools{display:flex;align-items:center;gap:8px;padding:9px 14px;border-bottom:1px solid var(--line2);
  flex-wrap:wrap}
#review .tools .note{color:var(--mut2);font-size:12px;margin-left:auto}

.btn{padding:5px 11px;border-radius:7px;border:1px solid var(--line);background:rgba(255,255,255,.03);
  font-size:12.5px;white-space:nowrap;transition:background .15s,border-color .15s}
.btn:hover{background:rgba(255,255,255,.07);border-color:rgba(255,255,255,.2)}
.btn:disabled{opacity:.45;cursor:not-allowed}
.btn.ok{color:#bff4de;border-color:rgba(95,211,163,.4);background:rgba(95,211,163,.12)}
.btn.bad{color:#ffc4cc;border-color:rgba(239,122,138,.4);background:rgba(239,122,138,.12)}
.btn.primary{color:#d9f2ff;border-color:rgba(110,200,255,.45);background:rgba(110,200,255,.16)}
.btn.primary:not(:disabled):hover{background:rgba(110,200,255,.26)}

#items{overflow:auto;min-height:120px;padding:4px 0}
.row{display:grid;grid-template-columns:42px minmax(0,1fr) auto;gap:10px;align-items:center;
  padding:7px 14px;border-bottom:1px solid var(--line2)}
.row:last-child{border-bottom:0}
.row:hover{background:rgba(255,255,255,.02)}
.row .seq{color:var(--mut2);font:12px var(--mono);text-align:right}
.row .body{min-width:0}
.row .target{font-size:13.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.row .sub{color:var(--mut2);font-size:11.5px;display:flex;gap:8px;margin-top:1px}
.row .kind{font:11px var(--mono);color:var(--mut);border:1px solid var(--line2);border-radius:5px;
  padding:0 5px;line-height:16px}
.row.signed .target::after{content:"· 已签";color:var(--ok);font-size:11px;margin-left:8px}
.row.vetoed .target::after{content:"· 已拒";color:var(--bad);font-size:11px;margin-left:8px}
.tracks{display:flex;gap:14px;align-items:center}
.track{display:flex;align-items:center;gap:5px}
.track .lbl{color:var(--mut2);font-size:11.5px;width:28px;text-align:right}
.pill{width:26px;height:22px;border-radius:6px;border:1px solid var(--line);display:inline-flex;
  align-items:center;justify-content:center;font-size:12px;color:var(--mut2);background:rgba(255,255,255,.02)}
.pill:hover{border-color:rgba(255,255,255,.24)}
.pill.on-ok{color:#0a1a12;background:var(--ok);border-color:var(--ok);font-weight:700}
.pill.on-bad{color:#22090d;background:var(--bad);border-color:var(--bad);font-weight:700}

.empty{padding:26px 16px;color:var(--mut);font-size:13px;line-height:1.8}
.empty code{font:12px var(--mono);color:#cfe3ff;background:rgba(110,200,255,.1);
  border:1px solid rgba(110,200,255,.2);border-radius:5px;padding:1px 6px;white-space:pre-wrap;
  word-break:break-all}

aside{display:flex;flex-direction:column;gap:16px}
.card{padding:14px}
.card .kv{display:flex;justify-content:space-between;gap:10px;font-size:12.5px;color:var(--mut);
  padding:3px 0}
.card .kv b{color:var(--fg);font-weight:600}
.card .kv .mono{font:12px var(--mono);color:#cfe3ff;word-break:break-all;text-align:right}
.card .sep{height:1px;background:var(--line2);margin:10px 0}
.card .actions{display:flex;gap:8px;margin-top:10px;flex-wrap:wrap}
.card .desc{color:var(--mut2);font-size:11.5px;margin-top:8px;line-height:1.7}
.card .result{margin-top:10px;display:none;font-size:12.5px;color:var(--mut);
  border-top:1px dashed var(--line2);padding-top:10px;line-height:1.9}
.card .result.on{display:block}
.card .result .mono{font:11.5px var(--mono);color:#cfe3ff;word-break:break-all}
footer{padding:0 20px 20px;color:var(--mut2);font-size:11.5px}
</style>
</head>
<body>
<header>
  <h1>本地评审台</h1>
  <div class="meta">
    <span>目录 <b id="mDir">—</b></span>
    <span>草稿 <b id="mItems">0</b> 条</span>
    <span>已签 <b id="mSigned">0</b></span>
    <span>定版 <b id="mVersion">—</b></span>
  </div>
  <span class="badge">本机 · 无口令</span>
</header>

<div id="banner">草稿在裁决之后变过 —— 这份裁决可能对不上当前草稿（定版会被拒绝）。请逐条重签一遍（已签的条目仍保留），或把草稿恢复成裁决时的那一份。</div>
<div id="error"><div class="t" id="errTitle"></div><div class="m" id="errMsg"></div><div class="h" id="errHint"></div></div>

<main>
  <section id="review">
    <h2>评审（核心）<span class="hint">两轨都给值才算签完</span></h2>
    <div class="bar">
      <div id="track"><div id="trackFill"></div></div>
      <div id="progressText">—</div>
    </div>
    <div class="tools">
      <button class="btn ok" id="btnAcceptAll">全选接受</button>
      <button class="btn bad" id="btnRejectAll">全选拒绝</button>
      <span class="note" id="bulkNote"></span>
    </div>
    <div id="items"></div>
    <div class="empty" id="emptyReview" style="display:none"></div>
  </section>

  <aside>
    <section>
      <h2>定版<span class="hint">冻结 · 不可改</span></h2>
      <div class="card">
        <div class="kv"><span>最新定版</span><b id="vSeq">—</b></div>
        <div class="kv"><span>时间</span><span class="mono" id="vTime">—</span></div>
        <div class="kv"><span>条数</span><b id="vItems">—</b></div>
        <div class="sep"></div>
        <div class="actions">
          <button class="btn primary" id="btnFinalize" disabled>定版</button>
        </div>
        <div class="desc" id="finalizeHint">签完才能定版。定版后不可改，要改就再定一版。</div>
      </div>
    </section>

    <section>
      <h2>导出<span class="hint">导出的是定版</span></h2>
      <div class="card">
        <div class="kv"><span>构建包</span><b id="bState">还没有</b></div>
        <div class="actions">
          <button class="btn primary" id="btnBundle" disabled>导出构建包</button>
        </div>
        <div class="result" id="bundleResult">
          <div><span id="bFile" class="mono"></span></div>
          <div><span id="bBytes"></span> · sha256 <span id="bSha" class="mono"></span></div>
          <div><a id="bDownload" href="#">下载 .ontokbundle</a></div>
        </div>
        <div class="desc">导出的是**定版**，不是草稿。包导出到 <code>.ontok/bundles/</code>，手动上传到 Studio。</div>
      </div>
    </section>
  </aside>
</main>

<footer>一个工作目录 = 一个项目 · 草稿 <code>out/proposal.json</code>（只读）· 裁决 <code>.ontok/decisions.json</code> · 定版 <code>.ontok/versions/vN/</code></footer>

<script>
"use strict";
var state = null;
var draft = null;
var busy = false;
var lastBundle = null;

function $(id){ return document.getElementById(id); }
function el(tag, cls, text){
  var n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined && text !== null) n.textContent = String(text);
  return n;
}
function fmtBytes(n){
  if (n === null || n === undefined) return "—";
  if (n < 1024) return String(n);
  if (n < 1024 * 1024) return (n / 1024).toFixed(1) + " KiB";
  return (n / (1024 * 1024)).toFixed(2) + " MiB";
}
function showError(title, msg, hint){
  $("errTitle").textContent = title || "出错了";
  $("errMsg").textContent = msg || "";
  $("errHint").textContent = hint || "";
  $("error").className = "on";
}
function clearError(){ $("error").className = ""; }
// 危险动作的**唯一**确认入口：每个动作都要写清「动作名 + 目标 + 后果」。
// ⛔ 不要把 window.confirm 散落在业务函数里 —— 判据按 guarded 规格的个数计（恰好 5 个），
// 并按「动作名 / 目标 / 后果」逐条对（`tests/test_req125_page.rs`）。
function guarded(spec, run){
  var msg = spec.action + "\n\n目标：" + spec.target + "\n后果：" + spec.after;
  if (!window.confirm(msg)) return false;
  run();
  return true;
}
function setBusy(on, note){
  busy = on;
  $("btnAcceptAll").disabled = on || !draft || !draft.items.length;
  $("btnRejectAll").disabled = on || !draft || !draft.items.length;
  $("bulkNote").textContent = note || "";
  renderExport();
}

function request(method, path, body){
  var opts = { method: method, headers: {} };
  if (body !== undefined){
    opts.headers["content-type"] = "application/json";
    opts.body = JSON.stringify(body);
  }
  return fetch(path, opts).then(function(res){
    return res.text().then(function(text){
      var data = null;
      try { data = text ? JSON.parse(text) : null; } catch (e) { data = null; }
      if (!res.ok){
        var err = new Error((data && data.message) || ("HTTP " + res.status));
        err.code = data && data.code;
        err.hint = data && data.hint;
        throw err;
      }
      return data;
    });
  });
}

function renderHeader(){
  if (!state) return;
  $("mDir").textContent = state.dir || "—";
  $("mItems").textContent = String(state.draft.items);
  $("mSigned").textContent = String(state.draft.signed);
  var v = state.latest_version;
  $("mVersion").textContent = v ? ("v" + v.seq) : "—";
  $("banner").className = state.draft.stale ? "on" : "";
}

function renderProgress(){
  var total = draft ? draft.progress.total : 0;
  var signed = draft ? draft.progress.signed : 0;
  var pct = total ? Math.round(signed * 100 / total) : 0;
  $("trackFill").style.width = pct + "%";
  $("progressText").textContent = total ? (signed + " / " + total + " 已签 · 还差 " + draft.progress.pending) : "还没有草稿";
}

function trackCell(item, track, label){
  var wrap = el("div", "track");
  var lbl = el("span", "lbl", label);
  var yes = el("button", "pill", "✓");
  var no = el("button", "pill", "✗");
  yes.title = label + "：接受";
  no.title = label + "：拒绝";
  var cur = item[track];
  if (cur === "accepted") yes.className = "pill on-ok";
  if (cur === "rejected") no.className = "pill on-bad";
  yes.addEventListener("click", function(){ decide(item.key, track, "accepted"); });
  no.addEventListener("click", function(){ decide(item.key, track, "rejected"); });
  wrap.appendChild(lbl); wrap.appendChild(yes); wrap.appendChild(no);
  return wrap;
}

function renderItems(){
  var box = $("items");
  while (box.firstChild) box.removeChild(box.firstChild);
  var empty = $("emptyReview");
  if (!draft || !draft.proposal){
    empty.style.display = "block";
    empty.textContent = "还没有草稿：先在工作目录里跑 " +
      "ontok-builder propose --schema out/schema.json --names out/table_names.json --out out/proposal.json";
    return;
  }
  if (!draft.items.length){
    empty.style.display = "block";
    empty.textContent = "草稿里一条条目都没有 —— 这份 proposal.json 的 items 是空数组。";
    return;
  }
  empty.style.display = "none";
  var frag = document.createDocumentFragment();
  draft.items.forEach(function(item){
    var row = el("div", "row");
    if (item.signed) row.className = item.biz === "rejected" || item.model === "rejected" ? "row signed vetoed" : "row signed";
    row.appendChild(el("div", "seq", item.seq));
    var body = el("div", "body");
    body.appendChild(el("div", "target", item.target));
    var sub = el("div", "sub");
    sub.appendChild(el("span", "kind", item.kind));
    if (item.from_type && item.to_type) sub.appendChild(el("span", null, item.from_type + " → " + item.to_type));
    if (item.reason) sub.appendChild(el("span", null, "理由：" + item.reason));
    body.appendChild(sub);
    row.appendChild(body);
    var tracks = el("div", "tracks");
    tracks.appendChild(trackCell(item, "biz", "业务"));
    tracks.appendChild(trackCell(item, "model", "建模"));
    row.appendChild(tracks);
    frag.appendChild(row);
  });
  box.appendChild(frag);
}

function renderExport(){
  var v = state && state.latest_version;
  var pending = draft ? draft.progress.pending : 0;
  var total = draft ? draft.progress.total : 0;
  if (v){
    $("vSeq").textContent = "v" + v.seq;
    $("vTime").textContent = v.created_at || "—";
    $("vItems").textContent = String(v.items);
  } else {
    $("vSeq").textContent = "—"; $("vTime").textContent = "—"; $("vItems").textContent = "—";
  }
  var fin = $("btnFinalize");
  var canFin = !!draft && total > 0 && pending === 0 && !busy;
  fin.disabled = !canFin;
  if (!draft || !draft.items || !draft.items.length){
    $("finalizeHint").textContent = "还没有草稿。先跑 propose 生成 out/proposal.json。";
  } else if (pending > 0){
    $("finalizeHint").textContent = "还差 " + pending + " 条没签完，签完才能定版。定版后不可改，要改就再定一版。";
  } else {
    $("finalizeHint").textContent = "可以定版。定版后不可改，要改就再定一版；草稿改过就交增量草稿（同一份全量草稿再定版会冲突）。";
  }
  var b = state && state.bundle;
  $("bState").textContent = v ? (b ? b.file : "还没有（先导出）") : "还没有（先定版）";
  $("btnBundle").disabled = !v || busy;
}

function setItem(item){
  if (!draft) return;
  for (var i = 0; i < draft.items.length; i++){
    if (draft.items[i].key === item.key){ draft.items[i] = item; break; }
  }
  draft.progress.signed = 0;
  draft.items.forEach(function(it){ if (it.signed) draft.progress.signed++; });
  draft.progress.pending = draft.progress.total - draft.progress.signed;
  draft.stale = false;
  if (state){
    state.draft.signed = draft.progress.signed;
    state.draft.pending = draft.progress.pending;
    state.draft.stale = false;
  }
  renderHeader(); renderProgress(); renderItems(); renderExport();
}

function decide(key, track, value){
  if (busy) return;
  clearError();
  var body = { key: key };
  body[track] = value;
  request("POST", "/api/decide", body).then(function(res){
    if (!draft) return;
    for (var i = 0; i < draft.items.length; i++){
      if (draft.items[i].key === key){
        var item = draft.items[i];
        item.biz = res.biz;
        item.model = res.model;
        item.signed = res.signed;
        setItem(item);
        return;
      }
    }
  }).catch(function(e){
    showError("裁决没写进去", e.message, e.hint || "");
  });
}

function bulkAccept(){
  if (busy || !draft) return;
  var list = draft.items.slice();
  guarded({
    action: "全选接受",
    target: "全部 " + list.length + " 条（业务轨 + 建模轨一起签）",
    after: "一次写入 " + list.length + " 条裁决；裁决可以逐条改回，但这是批量动作，请先确认草稿"
  }, function(){ runBulk(list, "accepted"); });
}

function bulkReject(){
  if (busy || !draft) return;
  var list = draft.items.slice();
  guarded({
    action: "全选拒绝",
    target: "全部 " + list.length + " 条（业务轨 + 建模轨一起否决）",
    after: "一次写入 " + list.length + " 条否决；被拒的条目不会进定版，可以逐条改回"
  }, function(){ runBulk(list, "rejected"); });
}

function runBulk(list, decision){
  var i = 0;
  clearError();
  setBusy(true, "正在签 0 / " + list.length);
  function step(){
    if (i >= list.length){
      setBusy(false, "已全选" + (decision === "accepted" ? "接受" : "拒绝") + " " + list.length + " 条");
      return;
    }
    var item = list[i++];
    request("POST", "/api/decide", { key: item.key, biz: decision, model: decision })
      .then(function(res){
        item.biz = res.biz; item.model = res.model; item.signed = res.signed;
        setItem(item);
        setBusy(true, "正在签 " + i + " / " + list.length);
        step();
      })
      .catch(function(e){
        setBusy(false, "签到第 " + i + " 条停了");
        showError("全选没做完", e.message, e.hint || "");
      });
  }
  step();
}

function finalize(){
  if (busy || !draft) return;
  if (draft.progress.pending > 0) return;
  guarded({
    action: "定版（冻结当前草稿）",
    target: "草稿 " + draft.progress.total + " 条 / 已签 " + draft.progress.signed + " 条",
    after: "定版后不可改，要改就再定一版（旧定版不跟着变）"
  }, function(){
    clearError();
    setBusy(true, "正在定版…");
    request("POST", "/api/finalize").then(function(res){
      setBusy(false, "");
      return load().then(function(){
        $("bundleResult").className = "result";
        $("bFile").textContent = "已定版 v" + res.seq + " · " + res.items + " 条";
        $("bBytes").textContent = "快照 sha256";
        $("bSha").textContent = res.snapshot_sha256 || "";
        $("bDownload").textContent = "下一步：导出构建包";
      });
    }).catch(function(e){
      setBusy(false, "");
      showError("定版失败", e.message, e.hint || "");
    });
  });
}

function exportBundle(){
  if (busy || !state || !state.latest_version) return;
  var seq = state.latest_version.seq;
  guarded({
    action: "导出构建包",
    target: "定版 v" + seq + "（" + state.latest_version.items + " 条）",
    after: "只导出定版、不导出草稿；包写进 .ontok/bundles/，之后手动上传到 Studio"
  }, function(){
    clearError();
    setBusy(true, "正在打包…");
    request("POST", "/api/bundle", { seq: seq }).then(function(res){
      setBusy(false, "");
      lastBundle = res;
      $("bundleResult").className = "result on";
      $("bFile").textContent = res.file;
      $("bBytes").textContent = fmtBytes(res.bytes) + "（" + res.bytes + "）";
      $("bSha").textContent = res.sha256;
      $("bDownload").textContent = "下载 " + res.file;
      $("bDownload").href = "/api/bundle/download?seq=" + res.seq;
      return request("GET", "/api/state").then(function(s){
        state = s; renderHeader(); renderExport();
      });
    }).catch(function(e){
      setBusy(false, "");
      showError("导出失败", e.message, e.hint || "");
    });
  });
}

function downloadBundle(ev){
  ev.preventDefault();
  if (!lastBundle) return;
  guarded({
    action: "下载构建包",
    target: "定版 v" + lastBundle.seq + " 的 " + lastBundle.file,
    after: "把包复制到本机下载目录（不改工作目录里的任何文件），之后手动上传到 Studio"
  }, function(){ window.location.href = $("bDownload").href; });
}

function load(){
  return request("GET", "/api/state").then(function(s){
    state = s;
    renderHeader();
    if (!s.draft.exists){
      draft = { items: [], progress: { total: 0, signed: 0, pending: 0 }, stale: false, proposal: null };
      renderProgress(); renderItems(); renderExport();
      return;
    }
    return request("GET", "/api/draft").then(function(d){
      draft = d;
      renderHeader(); renderProgress(); renderItems(); renderExport();
      $("bulkNote").textContent = draft.items.length ? "" : "草稿是空的";
    });
  }).catch(function(e){
    showError("读不出工作目录", e.message, e.hint || "");
  });
}

$("btnAcceptAll").addEventListener("click", bulkAccept);
$("btnRejectAll").addEventListener("click", bulkReject);
$("btnFinalize").addEventListener("click", finalize);
$("btnBundle").addEventListener("click", exportBundle);
$("bDownload").addEventListener("click", downloadBundle);
load();
</script>
</body>
</html>"####;

/// 页面响应必须带的那一份 CSP（`tests/test_req162_console.rs` 逐字对）。
///
/// `script-src` / `style-src` 的 `'unsafe-inline'` 是**单文件页面**的必要代价：
/// 没有构建产物，就没有 nonce / hash 可发。代价被三条反向约束收住：
/// 零外部资源（`default-src 'self'`）、同源请求（`connect-src 'self'`）、
/// 用户数据只走 `textContent`（没有字符串拼 HTML 的写点）。
pub const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; \
base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// 页面响应必须带的三个头（AC-011 的同一份口径）。
pub const SECURITY_HEADERS: [(&str, &str); 3] = [
    ("content-security-policy", CSP),
    ("x-content-type-options", "nosniff"),
    ("referrer-policy", "no-referrer"),
];

/// 把 [`SECURITY_HEADERS`] 变成 axum 的头部数组（类型转换只在这一处）。
pub fn security_headers() -> [(axum::http::HeaderName, axum::http::HeaderValue); 3] {
    use axum::http::{HeaderName, HeaderValue};
    [
        (
            HeaderName::from_static(SECURITY_HEADERS[0].0),
            HeaderValue::from_static(SECURITY_HEADERS[0].1),
        ),
        (
            HeaderName::from_static(SECURITY_HEADERS[1].0),
            HeaderValue::from_static(SECURITY_HEADERS[1].1),
        ),
        (
            HeaderName::from_static(SECURITY_HEADERS[2].0),
            HeaderValue::from_static(SECURITY_HEADERS[2].1),
        ),
    ]
}
