/* ══════════════════════════════════════════════════════════════
   Console 样机 · 只用真实接口的形状渲染（离线快照）
   ⛔ 不用 innerHTML（一律 createElement）；⛔ 无第三方依赖
   ══════════════════════════════════════════════════════════════ */
(function () {
  "use strict";

  var S = window.CONSOLE_SNAPSHOT || {};
  var $ = function (id) { return document.getElementById(id); };

  /* ── 小工具 ─────────────────────────────────────────── */
  function el(tag, attrs, text) {
    var n = document.createElement(tag);
    if (attrs) {
      Object.keys(attrs).forEach(function (k) {
        if (k === "class") n.className = attrs[k];
        else if (k === "html") n.textContent = attrs[k];
        else n.setAttribute(k, attrs[k]);
      });
    }
    if (text !== undefined && text !== null) {
      n.appendChild(document.createTextNode(String(text)));
    }
    return n;
  }
  function clear(n) { while (n.firstChild) n.removeChild(n.firstChild); return n; }
  function pill(text, cls) { return el("span", { "class": "pill " + (cls || "") }, text); }
  function fmtBytes(b) {
    if (b > 1024 * 1024) return (b / 1048576).toFixed(1) + " MB";
    if (b > 1024) return Math.round(b / 1024) + " KB";
    return b + " B";
  }
  function fmtTime(iso) {
    if (!iso) return "—";
    return String(iso).replace("T", " ").replace(/\.\d+Z?$/, "").replace("Z", "");
  }
  function kv(parent, pairs) {
    var dl = el("dl", { "class": "dl" });
    pairs.forEach(function (p) {
      dl.appendChild(el("dt", null, p[0]));
      var dd = el("dd", p[2] ? { "class": "mono" } : null, p[1]);
      dl.appendChild(dd);
    });
    parent.appendChild(dl);
    return dl;
  }
  /* 通用表格：cols = [{k,label,cls,render}] */
  function table(host, cols, rows, opts) {
    opts = opts || {};
    clear(host);
    var thead = el("thead"), tr = el("tr");
    cols.forEach(function (c) { tr.appendChild(el("th", null, c.label)); });
    thead.appendChild(tr);
    host.appendChild(thead);
    var tb = el("tbody");
    if (!rows.length) {
      var r0 = el("tr"), c0 = el("td", { "class": "empty", colspan: String(cols.length) },
        opts.empty || "没有数据");
      r0.appendChild(c0); tb.appendChild(r0);
    }
    rows.forEach(function (row) {
      var r = el("tr");
      if (opts.onRow) {
        r.style.cursor = "pointer";
        r.addEventListener("click", function () { opts.onRow(row, r); });
      }
      cols.forEach(function (c) {
        var td = el("td", c.cls ? { "class": c.cls } : null);
        var v = c.render ? c.render(row) : row[c.k];
        if (v && typeof v === "object" && v.nodeType) td.appendChild(v);
        else td.appendChild(document.createTextNode(v === undefined || v === null || v === "" ? "—" : String(v)));
        r.appendChild(td);
      });
      tb.appendChild(r);
    });
    host.appendChild(tb);
  }

  /* ── 数据整形 ───────────────────────────────────────── */
  var items = (S.catalog && S.catalog.items) || [];
  var classes = items.filter(function (i) { return i.kind === "class"; });
  var rels = items.filter(function (i) { return i.kind === "relation"; });
  var props = items.filter(function (i) { return i.kind === "property"; });
  var versions = (S.projects && S.projects.versions) || [];

  function accessKind(c) {
    var a = c.access || {};
    return a.kind || "db";
  }

  /* ══ 01 项目与版本 ══════════════════════════════════════ */
  function renderProjects() {
    var cards = clear($("proj-cards"));
    var frozen = versions.filter(function (v) { return v.frozen; }).length;
    [
      ["类", classes.length, "本版本"],
      ["关系", rels.length, "全部 unverified"],
      ["属性", props.length, "与 describe 一致"],
      ["版本", versions.length, frozen + " 个已冻结"]
    ].forEach(function (c) {
      var card = el("div", { "class": "card" });
      card.appendChild(el("div", { "class": "k" }, c[0]));
      card.appendChild(el("div", { "class": "v" }, c[1]));
      card.appendChild(el("div", { "class": "s" }, c[2]));
      cards.appendChild(card);
    });

    table($("tbl-versions"), [
      { label: "版本", cls: "key", render: function (v) { return "v" + v.version; } },
      {
        label: "状态", render: function (v) {
          return pill(v.frozen ? "已冻结" : "草稿", v.frozen ? "pill-frozen" : "pill-draft");
        }
      },
      { label: "冻结时间", cls: "dim", render: function (v) { return fmtTime(v.frozen_at); } },
      { label: "类", cls: "num", render: function (v) { return v.objects; } },
      { label: "关系", cls: "num", render: function (v) { return v.relations; } },
      { label: "快照", render: function (v) { return v.has_snapshot ? pill("有", "pill-ok") : pill("无", "pill-bad"); } },
      {
        label: "抽取来源", cls: "dim", render: function (v) {
          var e = v.extracted_by || {};
          return (e.agent || "—") + (e.round ? " · 第 " + e.round + " 轮" : "");
        }
      }
    ], versions, { empty: "还没有版本" });
  }

  /* ══ 02 评审 ═══════════════════════════════════════════ */
  var decisions = {};

  function reviewGroups() {
    /* 分组口径：**要不要人判断**。文档有据的概念 → 建议接受；关系全 unverified → 待确认。 */
    var accept = [], confirm = [];
    classes.forEach(function (c, i) {
      accept.push({
        id: "concept-" + (i + 1), track: "concept", name: c.name,
        detail: c.display_name || "", hint: "文档有据（" + c.property_count + " 属性）"
      });
    });
    rels.forEach(function (r, i) {
      var verified = r.evidence_level === "verified";
      var row = {
        id: "relation-" + (i + 1), track: "relation", name: r.name,
        detail: (r.from || "?") + " → " + (r.to || "?") + " · " + (r.cardinality || ""),
        hint: verified ? "已实测" : "无实测证据（文档推断）"
      };
      (verified ? accept : confirm).push(row);
    });
    return [
      { key: "accept", title: "建议接受", note: "文档有据 / 已实测", rows: accept, tone: "pill-ok" },
      { key: "confirm", title: "待人工确认", note: "evidence_level = unverified，全部 34 条关系都在这一组", rows: confirm, tone: "pill-draft" }
    ];
  }

  function renderReview() {
    var groups = reviewGroups();
    var tbl = $("tbl-review");
    clear(tbl);

    var cols = ["类型", "条目", "说明", "AI 建议", "决策", ""];
    var thead = el("thead"), htr = el("tr");
    cols.forEach(function (c) { htr.appendChild(el("th", null, c)); });
    thead.appendChild(htr);
    tbl.appendChild(thead);

    var tb = el("tbody");
    var total = 0, decided = 0;

    groups.forEach(function (g) {
      total += g.rows.length;
      /* 组头：跨整行，左侧色条 + 计数 */
      var gtr = el("tr", { "class": "group-row" });
      var td = el("td", { colspan: String(cols.length) });
      var bar = el("span", { "class": "group-bar " + (g.tone === "pill-ok" ? "ok" : "warn") });
      td.appendChild(bar);
      td.appendChild(el("span", { "class": "group-title" }, g.title));
      td.appendChild(el("span", { "class": "group-count" }, String(g.rows.length)));
      td.appendChild(el("span", { "class": "group-note" }, g.note));
      gtr.appendChild(td);
      tb.appendChild(gtr);

      g.rows.forEach(function (r) {
        var d = decisions[r.id];
        if (d) decided++;
        var tr = el("tr");
        tr.appendChild(el("td", null, pill(r.track, r.track === "concept" ? "pill-ok" : "pill-api")));
        tr.appendChild(el("td", { "class": "key" }, r.name));
        tr.appendChild(el("td", { "class": "dim" }, r.detail || "—"));
        tr.appendChild(el("td", { "class": "dim" }, r.hint));
        tr.appendChild(el("td", null, d === "accept" ? pill("接受", "pill-ok")
          : d === "reject" ? pill("驳回", "pill-bad") : pill("待定", "pill-draft")));
        var act = el("td");
        ["accept", "reject"].forEach(function (k) {
          var b = el("button", { "class": "btn btn-ghost", style: "height:22px;padding:0 8px;margin-left:4px" },
            k === "accept" ? "✓" : "✕");
          b.addEventListener("click", function (ev) {
            ev.stopPropagation();
            decisions[r.id] = decisions[r.id] === k ? undefined : k;
            renderReview();
          });
          act.appendChild(b);
        });
        tr.appendChild(act);
        tb.appendChild(tr);
      });
    });

    tbl.appendChild(tb);
    $("rev-tally").textContent = decided + " / " + total + " 已决";
    $("rev-all-yes").textContent = "全部接受（" + total + " 条）";
    $("rev-all-yes").onclick = function () {
      groups.forEach(function (g) { g.rows.forEach(function (r) { decisions[r.id] = "accept"; }); });
      renderReview();
    };
  }

  /* ══ 03 导出 ═══════════════════════════════════════════ */
  function renderExport() {
    var t = S.ttl_head || "";
    $("ex-out").textContent = t;
    $("ex-meta").textContent = fmtBytes(S.ttl_bytes || 0) + " · 客户视图";
  }

  /* ══ 04 声明编辑 ═══════════════════════════════════════ */
  var cfgState = { exclusions: null, policy: null, tenancy: null };
  var cfgCur = "exclusions";
  var CFG_DOC = {
    exclusions: {
      name: "exclusions.json",
      body: JSON.stringify({
        version: "1.0.0",
        tables: [{ table: "tmp_import", reason: "临时导入表，不进本体" }],
        main_only: [{ table: "region", reason: "地域字典，只作为主表维度" }],
        publicDimensions: [{ column: "internal_ip", reason: "内网地址不对客户暴露" }]
      }, null, 2)
    },
    policy: {
      name: "query-policy.json",
      body: JSON.stringify({ version: "1.0.0", max_rows: 200, max_concurrency: 4, tables: {} }, null, 2)
    },
    tenancy: {
      name: "tenancy.json",
      body: JSON.stringify({ version: "1.0.0", tables: {} }, null, 2)
    }
  };
  function cfgChecks(kind, text) {
    var checks = [];
    var ok = false;
    try { JSON.parse(text); ok = true; } catch (e) { ok = false; }
    checks.push(["JSON 能解析", ok, ok ? "" : "第 " + "? 行"]);
    checks.push(["顶层有 version", /"version"\s*:/.test(text), ""]);
    if (kind === "exclusions") {
      var needsReason = /main_only|publicDimensions/.test(text);
      checks.push(["main_only / publicDimensions 带 reason",
        !needsReason || /"reason"\s*:\s*"[^"]+"/.test(text), "没有 reason 会被 400 拒绝"]);
    }
    checks.push(["未提交（样机）", false, "真实现里保存前调用后端校验"]);
    return checks;
  }
  function renderConfig() {
    var spec = CFG_DOC[cfgCur];
    $("cfg-name").textContent = spec.name;
    var body = $("cfg-body");
    body.value = cfgState[cfgCur] === null ? spec.body : cfgState[cfgCur];
    $("cfg-state").textContent = "未创建";
    $("cfg-state").className = "pill pill-draft";
    drawChecks();   /* ⚠️ 监听器在 init 里只绑一次，别在这里重复绑 */
  }
  function drawChecks() {
    var host = clear($("cfg-checks"));
    cfgChecks(cfgCur, $("cfg-body").value).forEach(function (c) {
      var d = el("div", { "class": "chk " + (c[1] ? "ok" : "bad") });
      d.appendChild(el("span", null, c[0]));
      if (c[2]) d.appendChild(el("span", { "class": "why" }, " — " + c[2]));
      host.appendChild(d);
    });
  }

  /* ══ 05 审计 ═══════════════════════════════════════════ */
  function renderAudit() {
    var entries = (S.audit && S.audit.entries) || [];
    $("audit-count").textContent = (S.audit && S.audit.count || 0) + " 条";
    table($("tbl-audit"), [
      { label: "时间", cls: "dim", render: function (e) { return fmtTime(e.at || e.ts); } },
      { label: "动作", cls: "key", render: function (e) { return e.action || e.op; } },
      { label: "对象", render: function (e) { return e.target || e.project || "—"; } },
      { label: "令牌", cls: "dim", render: function (e) { return (e.token_sha12 || "—") + "…"; } },
      { label: "结果", render: function (e) { return e.ok ? pill("成功", "pill-ok") : pill("失败", "pill-bad"); } }
    ], entries, { empty: "还没有写操作（快照是只读抓的）" });
  }

  /* ══ 06 查询台 ═════════════════════════════════════════ */
  var Q = {
    a: { q: "SELECT ?c ?name WHERE {\n  ?c a :cvm_instance ;\n     :name ?name .\n} LIMIT 5", r: function () { return S.q_api; }, m: "200 · JSON-LD · 有 @pending" },
    b: { q: "SELECT ?c WHERE {\n  ?c a :account\n} LIMIT 5", r: function () { return S.q_db503; }, m: "503 · NO_DATA_SOURCE" },
    c: { q: "SELECT ?c WHERE { ?c a :cbs_disk } LIMIT 1", r: function () {
        return { "@context": { "type": "@type" }, "@graph": [], "meta": { complete: true, note: "样机：计数查询" } };
      }, m: "200 · 空图（计数）" }
  };
  function renderQuery(key) {
    var s = Q[key];
    $("q-body").value = s.q;
    $("q-out").textContent = JSON.stringify(s.r(), null, 2);
    $("q-meta").textContent = s.m;
  }

  /* ══ 07 目录浏览 ═══════════════════════════════════════ */
  function renderCatalog(filter) {
    var f = (filter || "").toLowerCase();
    var rows = classes.filter(function (c) {
      if (!f) return true;
      return (c.name || "").toLowerCase().indexOf(f) >= 0 ||
        (c.display_name || "").toLowerCase().indexOf(f) >= 0 ||
        (c.table || "").toLowerCase().indexOf(f) >= 0 ||
        String(c.description || "").toLowerCase().indexOf(f) >= 0;
    });
    $("cat-count").textContent = rows.length + " / " + classes.length + " 类";
    table($("tbl-catalog"), [
      { label: "类", cls: "key", render: function (c) { return c.name; } },
      { label: "中文名", render: function (c) { return c.display_name || "—"; } },
      { label: "属性", cls: "num", render: function (c) { return c.property_count; } },
      { label: "关系", cls: "num", render: function (c) { return c.relation_count; } },
      {
        label: "取数", render: function (c) {
          var k = accessKind(c);
          return pill(k === "api" ? "API 源" : "库源", k === "api" ? "pill-api" : "pill-db");
        }
      },
      {
        label: "证据", render: function (c) {
          var rels = relsOfClass(c.name);
          if (!rels.length) return el("span", { "class": "dim" }, "—");
          var verified = rels.filter(function (r) { return r.evidence_level === "verified"; }).length;
          return pill(verified ? verified + "/" + rels.length + " 已验" : "全部未验",
            verified ? "pill-ok" : "pill-draft");
        }
      },
      { label: "说明", cls: "dim", render: function (c) { return String(c.description || "").slice(0, 56); } }
    ], rows, { empty: "没有匹配的类", onRow: openDrawer });
  }

  function propsOfClass(name) {
    return props.filter(function (p) { return p["class"] === name; });
  }
  function relsOfClass(name) {
    return rels.filter(function (r) { return r.from === name || r.to === name; });
  }
  function openDrawer(c) {
    $("dr-title").textContent = c.name + (c.display_name ? " · " + c.display_name : "");
    var host = clear($("dr-body"));
    kv(host, [
      ["name", c.name, true],
      ["中文名", c.display_name || "—"],
      ["表 / 主键", (c.table || "—") + (c.primary_key ? " / " + c.primary_key : "")],
      ["取数", accessKind(c) === "api" ? "API 源（平台不执行，给 get 配方）" : "库源（SPARQL 可查）"],
      ["规模", c.property_count + " 属性 · " + c.relation_count + " 关系"],
      ["说明", c.description || "—"]
    ]);

    var ps = propsOfClass(c.name);
    if (ps.length) {
      host.appendChild(el("div", { "class": "subhead" }, "属性 " + ps.length));
      var list = el("div", { "class": "list" });
      ps.slice(0, 40).forEach(function (x) {
        var r = el("div", { "class": "row2" });
        r.appendChild(el("span", { "class": "k" }, x.name));
        r.appendChild(el("span", { "class": "t" }, (x.datatype || "") + (x.enum_values && x.enum_values.length ? " · 枚举 " + x.enum_values.length : "")));
        list.appendChild(r);
      });
      if (ps.length > 40) list.appendChild(el("div", { "class": "row2" }, "…还有 " + (ps.length - 40) + " 个"));
      host.appendChild(list);
    }

    var rs = relsOfClass(c.name);
    if (rs.length) {
      host.appendChild(el("div", { "class": "subhead" }, "关系 " + rs.length));
      var rl = el("div", { "class": "list" });
      rs.forEach(function (r) {
        var r2 = el("div", { "class": "row2" });
        r2.appendChild(el("span", { "class": "k" }, r.name));
        r2.appendChild(el("span", { "class": "t" }, (r.from || "?") + " → " + (r.to || "?") + " · " + (r.evidence_level || "")));
        rl.appendChild(r2);
      });
      host.appendChild(rl);
    }

    if ($("drawer").classList) {
      $("drawer").classList.add("is-on");
      $("drawer").setAttribute("aria-hidden", "false");
    }
  }

  /* ══ 导航 / 顶部 ═══════════════════════════════════════ */
  var rendered = {};
  function show(name) {
    document.querySelectorAll(".view").forEach(function (v) {
      v.classList.toggle("is-on", v.id === "v-" + name);
    });
    document.querySelectorAll(".nav-item").forEach(function (b) {
      b.classList.toggle("is-on", b.getAttribute("data-view") === name);
    });
    if (!rendered[name]) {
      rendered[name] = true;
      if (name === "projects") renderProjects();
      else if (name === "review") renderReview();
      else if (name === "export") renderExport();
      else if (name === "config") renderConfig();
      else if (name === "audit") renderAudit();
      else if (name === "query") renderQuery("a");
      else if (name === "catalog") renderCatalog("");
    }
  }

  function init() {
    // 头部
    $("h-service").textContent = (S.health && S.health.service) || "—";
    $("h-version").textContent = (S.health && S.health.version) || "—";
    $("h-source").textContent = "快照 " + String(S.captured_at || "").slice(5, 16);

    var projSel = $("proj");
    projSel.appendChild(el("option", null, (S.projects && S.projects.project) || "cloud-resource"));
    var verSel = $("ver");
    versions.slice().reverse().forEach(function (v) {
      var o = el("option", { value: String(v.version) }, "v" + v.version + (v.frozen ? " · 已冻结" : ""));
      if (v.version === 3) o.selected = true;
      verSel.appendChild(o);
    });
    var cur = versions.filter(function (v) { return v.version === 3; })[0] || versions[versions.length - 1];
    var badge = $("frozen-badge");
    badge.textContent = cur && cur.frozen ? "已冻结" : "草稿";
    badge.className = "pill " + (cur && cur.frozen ? "pill-frozen" : "pill-draft");

    document.querySelectorAll(".nav-item").forEach(function (b) {
      b.addEventListener("click", function () { show(b.getAttribute("data-view")); });
    });
    $("dr-close").addEventListener("click", function () {
      $("drawer").classList.remove("is-on");
      $("drawer").setAttribute("aria-hidden", "true");
    });
    document.addEventListener("keydown", function (e) {
      if (e.key === "Escape") {
        $("drawer").classList.remove("is-on");
        $("drawer").setAttribute("aria-hidden", "true");
      }
    });

    $("cfg-tabs").addEventListener("click", function (e) {
      var b = e.target.closest ? e.target.closest(".tab") : null;
      if (!b) return;
      document.querySelectorAll("#cfg-tabs .tab").forEach(function (t) { t.classList.remove("is-on"); });
      b.classList.add("is-on");
      cfgCur = b.getAttribute("data-cfg");
      renderConfig();
    });

    $("cat-filter").addEventListener("input", function () { renderCatalog(this.value); });
    $("cfg-body").addEventListener("input", function () { drawChecks(); });
    $("q-sample").addEventListener("change", function () { renderQuery(this.value); });
    $("q-run").addEventListener("click", function () { renderQuery($("q-sample").value); });
    $("ex-run").addEventListener("click", renderExport);
    $("btn-reload").addEventListener("click", function () { rendered = {}; show(curView()); });
    $("btn-export-ttl").addEventListener("click", function () { show("export"); });

    show("projects");
  }

  function curView() {
    var b = document.querySelector(".nav-item.is-on");
    return b ? b.getAttribute("data-view") : "projects";
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
