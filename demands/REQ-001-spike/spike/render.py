"""候选本体一页纸（REQ-001 · AC-005 的交付物）

**这是整个 spike 的意义所在。**
判据不是「模型输出得像不像本体」，而是「客户看一眼能不能指出哪里对、哪里不对」。

设计：刻意做成**无术语**的页面。客户不关心主键与基数，他们只关心
「这些是你们说的东西吗」。证据以「哪张表的哪一列」呈现，而不是外键关系图。

产物是自包含 HTML，零外部依赖，可直接双击打开。
"""

from __future__ import annotations

import html
import json
from datetime import datetime

from core import Candidate, Evidence, Schema, VerificationResult

_CSS = """
:root{--ink:#161513;--paper:#f2f2f0;--navy:#082d4f;--card:#fdfdfa;
--mut:#6f6c66;--hair:#c9c6c0;--wash:#f4f7fa;--ok:#1a6b3c;--bad:#9b2226;
--mono:"SF Mono",Menlo,Monaco,Consolas,monospace;
--sans:-apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif}
*{margin:0;padding:0;box-sizing:border-box}
body{background:var(--paper);color:var(--ink);font-family:var(--sans);line-height:1.75;
padding:2.5rem 1.5rem}
.wrap{max-width:900px;margin:0 auto}
.eyebrow{font-family:var(--mono);font-size:.7rem;letter-spacing:.2em;
text-transform:uppercase;color:var(--mut);margin-bottom:.6rem}
h1{font-size:1.9rem;font-weight:600;letter-spacing:-.01em;margin-bottom:.4rem}
.sub{color:var(--mut);font-size:.95rem;margin-bottom:1.6rem}
.howto{background:var(--wash);border-left:3px solid var(--navy);padding:1rem 1.2rem;
margin:0 0 2rem;font-size:.95rem}
.howto strong{color:var(--navy)}
.stats{display:flex;gap:.6rem;flex-wrap:wrap;margin:0 0 2rem}
.stat{border:1px solid var(--hair);background:var(--card);padding:.7rem 1rem;min-width:120px}
.stat .n{font-size:1.5rem;font-weight:600;font-family:var(--mono)}
.stat .l{font-size:.75rem;color:var(--mut)}
.stat.ok .n{color:var(--ok)} .stat.bad .n{color:var(--bad)}
.entity{background:var(--card);border:1px solid var(--hair);margin-bottom:1.1rem}
.entity.dropped{opacity:.62;border-style:dashed}
.ehd{display:flex;align-items:baseline;gap:.7rem;padding:.9rem 1.1rem;
border-bottom:1px solid var(--hair);flex-wrap:wrap}
.ehd .nm{font-size:1.15rem;font-weight:600}
.ehd .key{font-family:var(--mono);font-size:.7rem;color:var(--mut)}
.ehd .src{font-family:var(--mono);font-size:.72rem;color:var(--mut);margin-left:auto}
.alias{font-size:.8rem;color:var(--mut)}
.alias b{color:var(--ink);font-weight:500}
table{width:100%;border-collapse:collapse;font-size:.9rem}
th,td{text-align:left;padding:.45rem .6rem;border-bottom:1px solid var(--hair);
vertical-align:top}
th{font-size:.75rem;color:var(--mut);font-weight:600;background:var(--wash)}
tr:last-child td{border-bottom:none}
td.ev{font-family:var(--mono);font-size:.78rem;color:var(--navy)}
td.pk{font-family:var(--mono);font-size:.78rem;color:var(--mut)}
td.req{font-family:var(--mono);font-size:.78rem}
td.req.y{color:var(--bad)} td.req.n{color:var(--mut)}
.reason{font-family:var(--mono);font-size:.78rem;color:var(--bad)}
.tag{display:inline-block;font-family:var(--mono);font-size:.66rem;
padding:.1rem .45rem;border:1px solid var(--hair);color:var(--mut);margin-left:.4rem}
.tag.ok{border-color:var(--ok);color:var(--ok)}
.tag.bad{border-color:var(--bad);color:var(--bad)}
h2{font-size:1.15rem;font-weight:600;margin:2.2rem 0 .8rem;padding-bottom:.4rem;
border-bottom:1px solid var(--ink)}
footer{margin-top:3rem;padding-top:1rem;border-top:1px solid var(--hair);
font-family:var(--mono);font-size:.7rem;color:var(--mut)}
@media print{body{background:#fff;padding:1rem}}
"""


def _e(s: str) -> str:
    return html.escape(str(s))


def render_onepager(
    schema: Schema,
    kept: list[Candidate],
    dropped: list[tuple[Candidate, str]],
    stats: dict,
    project: str = "客户环境",
) -> str:
    """生成候选本体一页纸。"""
    total = stats["total"]
    rate_pct = round(stats["drop_rate"] * 100)
    verdict_ok = stats["drop_rate"] < 0.30
    verdict_mid = 0.30 <= stats["drop_rate"] < 0.50

    parts: list[str] = []
    parts.append("<!DOCTYPE html><html lang=\"zh-CN\"><head><meta charset=\"UTF-8\">")
    parts.append("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">")
    parts.append("<title>候选本体一页纸 · 抽取可信度验证</title>")
    parts.append(f"<style>{_CSS}</style></head><body><div class=\"wrap\">")

    parts.append("<p class=\"eyebrow\">抽取可信度验证 · REQ-001</p>")
    parts.append(f"<h1>候选本体一页纸</h1>")
    parts.append(
        f"<p class=\"sub\">来源：{_e(project)} · 共扫描 {len(schema.tables)} 张表 · "
        f"生成于 {datetime.now():%Y-%m-%d %H:%M}</p>"
    )

    # 给客户看的判读指引 —— 这一段是 AC-005 的核心
    parts.append(
        "<div class=\"howto\"><strong>请这样看这份清单：</strong>"
        "逐个看下面每个「对象」。问自己三个问题 ——<br>"
        "① <strong>这东西你们内部管它叫什么？</strong>（叫法不同请直接改）<br>"
        "② <strong>这里面的字段，有没有你们真实关心的漏了，或者不该有的？</strong><br>"
        "③ <strong>有没有某个对象，根本不存在，或者根本不该分开？</strong><br>"
        "<span style=\"color:var(--mut)\">"
        "不需要懂数据库结构。看到「对」或「不对」就够了，具体怎么改由 FDE 记录。"
        "</span></div>"
    )

    # 统计
    parts.append("<div class=\"stats\">")
    parts.append(
        f"<div class=\"stat\"><div class=\"n\">{stats['kept']}</div>"
        f"<div class=\"l\">通过验证</div></div>"
    )
    parts.append(
        f"<div class=\"stat {'bad' if stats['dropped'] else ''}\">"
        f"<div class=\"n\">{stats['dropped']}</div>"
        f"<div class=\"l\">证据不实被拦下</div></div>"
    )
    cls = "ok" if verdict_ok else ("bad" if verdict_mid else "")
    parts.append(
        f"<div class=\"stat {cls}\"><div class=\"n\">{rate_pct}%</div>"
        f"<div class=\"l\">幻觉率（门槛 &lt;30%）</div></div>"
    )
    parts.append(
        f"<div class=\"stat\"><div class=\"n\">{len(schema.tables)}</div>"
        f"<div class=\"l\">扫描的表</div></div>"
    )
    parts.append("</div>")

    by_reason = stats.get("by_reason", {})
    if by_reason:
        desc = {
            "table_not_found": "编造了不存在的表",
            "column_not_found": "编造了不存在的列",
            "no_evidence": "没给任何来源",
            "duplicate_key": "与已有类型重复",
        }
        parts.append("<h2>被拦下的候选 —— 幻觉明细</h2>")
        parts.append(
            "<p class=\"sub\" style=\"margin-bottom:.6rem\">"
            "这些是<strong>机器提出、但依据不成立</strong>的候选，已被代码验证拦下。"
            "列出来是为了让幻觉率可被审。</p>"
        )
        parts.append(
            "<table><thead><tr><th>原因</th><th>次数</th><th>说明</th></tr></thead><tbody>"
        )
        for k, v in sorted(by_reason.items(), key=lambda kv: -kv[1]):
            parts.append(
                f"<tr><td class=\"ev\">{_e(k)}</td><td>{v}</td>"
                f"<td>{_e(desc.get(k, ''))}</td></tr>"
            )
        parts.append("</tbody></table>")

    # 通过验证的候选
    parts.append("<h2>候选对象（请逐个判读）</h2>")
    if not kept:
        parts.append("<p class=\"sub\">没有候选通过验证。</p>")
    for c in kept:
        parts.append(_entity_block(c, schema, dropped=False))

    # 被丢弃的
    if dropped:
        parts.append("<h2>已拦下（仅供参考，不必判读）</h2>")
        for c, reason in dropped:
            parts.append(_entity_block(c, schema, dropped=True, reason=reason))

    parts.append(
        f"<footer>抽取可信度 spike · REQ-001 · "
        f"共 {total} 条候选，保留 {stats['kept']} 条 · "
        "所有证据已由代码回查真实 schema 验证</footer>"
    )
    parts.append("</div></body></html>")
    return "".join(parts)


def _entity_block(c: Candidate, schema: Schema, dropped: bool, reason: str = "") -> str:
    cls = "entity dropped" if dropped else "entity"
    b = [f"<div class=\"{cls}\"><div class=\"ehd\">"]

    b.append(f"<span class=\"nm\">{_e(c.display_name)}</span>")
    b.append(f"<span class=\"key\">{_e(c.key)}</span>")
    if dropped:
        b.append("<span class=\"tag bad\">已拦下</span>")
    if c.parent:
        b.append(f"<span class=\"tag\">属于：{_e(c.parent)}</span>")
    b.append(f"<span class=\"src\">来自表 {_e(c.source_table)}</span>")
    b.append("</div>")

    if dropped and reason:
        b.append(
            f"<div style=\"padding:.7rem 1.1rem\"><span class=\"reason\">{_e(reason)}</span></div>"
        )
        return "".join(b)

    if c.aliases:
        b.append(
            f"<div class=\"alias\" style=\"padding:.6rem 1.1rem 0\">"
            f"你们的其他叫法：{'、'.join(_e(a) for a in c.aliases) or '（未提供）'}</div>"
        )

    # 属性表：把「来源列」显式列出来，客户据此判断对错
    # 注意：props 必须以 **列名** 为键 —— 下面就是按列名迭代的
    props: dict[str, dict] = {}
    for p in c.properties:
        if p.get("column"):
            props[p["column"]] = p
    cols: list[str] = []
    for p in c.properties:
        cn = p.get("column")
        if cn and cn not in cols:
            cols.append(cn)
    for e in c.evidence:
        if e.column and e.column not in cols:
            cols.append(e.column)

    t = schema.table(c.source_table)
    pk = set(t.primary_key) if t else set()

    b.append("<table><thead><tr><th>属性</th><th>必填</th><th>取自哪一列</th><th>这一列实际长什么样</th></tr></thead><tbody>")
    for cn in cols:
        p = props.get(cn, {})
        prop_name = p.get("name") or cn
        nullable = p.get("nullable", True)
        col = t.column_map.get(cn) if t else None
        pkmark = " <span class=\"tag\">主键</span>" if cn in pk else ""
        sample = _sample_for(t, cn)
        req = "可选" if nullable else '<span class="req y">必填</span>'
        b.append(
            f"<tr><td><strong>{_e(prop_name)}</strong>{pkmark}</td>"
            f"<td class=\"req\">{req}</td>"
            f"<td class=\"ev\">{_e(cn)}</td>"
            f"<td class=\"pk\">{_e(sample)}</td></tr>"
        )
    b.append("</tbody></table>")

    # 证据说明
    claims = [e for e in c.evidence if e.claim]
    if claims:
        b.append(
            "<div class=\"alias\" style=\"padding:.6rem 1.1rem .8rem\">"
            "<strong>机器的推断依据：</strong>"
            + "；".join(f"<em>{_e(e.claim)}</em>（{_e(e.table)}.{_e(e.column)}）" for e in claims)
            + "</div>"
        )

    b.append("</div>")
    return "".join(b)


def _sample_for(t, col: str, n: int = 2) -> str:
    """取样值 —— **必须走与脱敏相同的规则**。

    曾经的 bug：这里直接读 `t.sample_rows`（原始行），绕过了 desensitize，
    导致证件号 / 手机号原样出现在给客户看的页面上。
    """
    if not t or not t.sample_rows:
        return "（无样本）"
    if col not in t.column_map:
        return "—"
    column = t.column_map[col]
    idx = t.column_names.index(col)
    if column.is_sensitive:
        return f"«已脱敏»"
    from core import _clip

    vals = [_clip(r[idx], 24) for r in t.sample_rows[:n] if r[idx] is not None]
    return "、".join(vals) if vals else "—"


# ──────────────────────────────────────────────────────────
# 交付物 2 · 业务概念一页纸（这才是「本体」）
# ──────────────────────────────────────────────────────────

_CONCEPT_CSS = _CSS + """
.badge{font-family:var(--mono);font-size:.66rem;padding:.1rem .45rem;
border:1px solid var(--navy);color:var(--navy);margin-left:.4rem}
.badge.merge{background:var(--wash)}
.badge.split{background:#fbf3f3;border-color:var(--bad);color:var(--bad)}
.sig{font-size:.82rem;color:var(--mut);margin-top:.2rem}
.sig code{font-size:.78rem}
.was{font-size:.8rem;color:var(--mut);margin-top:.3rem}
.was s{color:var(--bad);text-decoration:none;opacity:.75}
.share{font-family:var(--mono);font-size:.72rem;color:var(--navy)}
"""


def render_concepts(
    schema: "Schema",
    clusters: list,
    stats: dict,
    project: str = "客户环境",
    scope: str = "",
    links: list | None = None,
) -> str:
    """业务概念一页纸 —— 把逐表候选聚合成业务对象后给客户判读。

    与 `render_onepager` 的区别：
      - onepager   逐表候选，**审幻觉率用**（给 FDE）
      - concepts   业务对象，**给客户判读**（AC-005 主交付物）
    """
    from core import _clip

    total_members = sum(len(c.members) for c in clusters)
    merged = [c for c in clusters if len(c.members) > 1]

    p: list[str] = []
    p.append("<!DOCTYPE html><html lang=\"zh-CN\"><head><meta charset=\"UTF-8\">")
    p.append("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">")
    p.append("<title>业务概念一页纸 · 抽取可信度验证</title>")
    p.append(f"<style>{_CONCEPT_CSS}</style></head><body><div class=\"wrap\">")

    p.append("<p class=\"eyebrow\">抽取可信度验证 · REQ-001 · 业务概念</p>")
    p.append("<h1>你们的业务对象</h1>")
    p.append(
        f"<p class=\"sub\">来源：{_e(project)} · 扫描 {len(schema.tables)} 张表 · "
        f"生成于 {datetime.now():%Y-%m-%d %H:%M}</p>"
    )
    if scope:
        p.append(f"<p class=\"sub\"><strong>本轮范围：</strong>{_e(scope)}</p>")

    p.append(
        "<div class=\"howto\"><strong>这份清单不是数据库翻译，是机器把散落的表"
        "归并成你们口中的『东西』之后的结果。</strong><br>"
        "请逐个看，问自己：<br>"
        "① <strong>这个对象，你们内部叫什么？</strong>（叫法不同直接改）<br>"
        "② <strong>属性有没有该有的漏了、不该有的多了？</strong><br>"
        "③ <strong>有没有根本不该存在的，或者本该拆开的？</strong><br>"
        "<span style=\"color:var(--mut)\">下面是机器的判断依据，"
        "不同意的地方直接说，不需要懂数据库。</span></div>"
    )

    # 统计
    p.append("<div class=\"stats\">")
    p.append(f"<div class=\"stat\"><div class=\"n\">{len(clusters)}</div>"
             f"<div class=\"l\">业务对象</div></div>")
    p.append(f"<div class=\"stat\"><div class=\"n\">{len(schema.tables)}</div>"
             f"<div class=\"l\">原始表</div></div>")
    p.append(f"<div class=\"stat\"><div class=\"n\">{len(merged)}</div>"
             f"<div class=\"l\">多表归并</div></div>")
    p.append(f"<div class=\"stat\"><div class=\"n\">{total_members}</div>"
             f"<div class=\"l\">被归并的候选</div></div>")
    nlinks = sum(1 for l in (links or []) if l.verdict == "keep")
    if links is not None:
        p.append(f"<div class=\"stat\"><div class=\"n\">{nlinks}</div>"
                 f"<div class=\"l\">对象间关系</div></div>")
    p.append("</div>")

    if merged:
        p.append("<div class=\"note\"><span class=\"lbl\">机器做过的归并</span>")
        p.append(f"<p>下面 {len(merged)} 组表被判定为同一个业务对象的不同形态"
                 f"（备份表、快照表、报表表等），属性已跨表聚合。</p>")
        p.append("<p><strong>请重点检查这组合并是否正确</strong> —— "
                 "结构相同的表有时语义相反（例如黑名单与白名单）。</p></div>")

    if links is not None:
        p.append(render_links(links, {}))

    p.append("<h2>业务对象清单（请逐个判读）</h2>")
    for c in clusters:
        prim = c.primary
        p.append('<div class="entity">')
        p.append('<div class="ehd">')
        p.append(f'<span class="nm">{_e(prim.display_name)}</span>')
        p.append(f'<span class="key">{_e(prim.key)}</span>')
        if len(c.members) > 1:
            if c.verdict == "merge":
                p.append(f'<span class="badge merge">模型判定：合并 {len(c.members)} 张表</span>')
            elif c.verdict == "split":
                p.append('<span class="badge split">模型判定：拆开</span>')
            else:
                p.append(f'<span class="badge">机器建议合并 {len(c.members)} 张，待你确认</span>')
        p.append(f'<span class="src">{"、".join(_e(t) for t in c.tables)}</span>')
        p.append("</div>")

        if c.reason:
            p.append(f'<div class="sig"><strong>判断理由：</strong>{_e(c.reason)}</div>')
        if c.signals:
            p.append(f'<div class="sig">机器的同源信号：'
                     + "；".join(_e(s) for s in c.signals[:3]) + "</div>")
        if c.related_to:
            p.append(f'<div class="sig">与 <strong>{_e(c.related_to)}</strong> '
                     f'是同类但不同的两个东西（兄弟关系）</div>')

        if prim.aliases:
            p.append(f'<div class="alias" style="padding:.5rem 1.1rem 0">'
                     f"你们的其他叫法：{_e('、'.join(prim.aliases))}</div>")

        props = c.merged_properties
        t0 = schema.table(prim.source_table)
        pk = set(t0.primary_key) if t0 else set()
        p.append("<table><thead><tr><th>属性</th><th>取自哪一列</th>"
                 "<th>共有情况</th><th>这一列长什么样</th></tr></thead><tbody>")
        for pr in props:
            col = pr["column"]
            also = pr["also_in"]
            share = (f'<span class="share">{len(also)} 张表都有</span>'
                     if len(also) > 1 else
                     f'<span class="dim">{_e(also[0])}</span>')
            sample = _sample_for(t0, col) if t0 else "—"
            pkmark = ' <span class="tag">主键</span>' if col in pk else ""
            p.append(
                f"<tr><td><strong>{_e(pr['name'])}</strong>{pkmark}</td>"
                f"<td class=\"ev\">{_e(col)}</td><td>{share}</td>"
                f"<td class=\"pk\">{_e(sample)}</td></tr>"
            )
        p.append("</tbody></table>")

        if len(c.members) > 1:
            p.append('<div class="was">归并前机器是这么分的（错了就说）：'
                     + "；".join(f"{_e(m.source_table)}→{_e(m.display_name)}"
                                 for m in c.members) + "</div>")
        p.append("</div>")

    p.append(
        f"<footer>业务概念一页纸 · REQ-001 · {len(schema.tables)} 张表 → "
        f"{len(clusters)} 个业务对象 · 幻觉率 {stats['drop_rate']:.1%}"
        "（证据已由代码回查真实 schema 验证）</footer>"
    )
    p.append("</div></body></html>")
    return "".join(p)


_LINK_CSS = _CSS + """
.lk{display:flex;align-items:stretch;gap:0;margin-bottom:.6rem;
border:1px solid var(--hair);background:var(--card)}
.lk.drop{opacity:.6;border-style:dashed}
.lk .node{flex:1 1 0;padding:.6rem .8rem;font-size:.92rem;min-width:9em}
.lk .node.to{background:var(--wash)}
.lk .node b{display:block;font-size:.95rem;white-space:nowrap;overflow:hidden;
text-overflow:ellipsis}
.lk .node span{font-family:var(--mono);font-size:.7rem;color:var(--mut);
white-space:nowrap;overflow:hidden;text-overflow:ellipsis;display:block}
.lk .arrow{flex:0 0 auto;display:flex;flex-direction:column;align-items:center;
justify-content:center;padding:.5rem .9rem;background:#fff;
border-left:1px solid var(--hair);border-right:1px solid var(--hair)}
.lk .arrow .sem{font-size:.85rem;font-weight:600;color:var(--navy);white-space:nowrap}
.lk .arrow .card2{font-family:var(--mono);font-size:.68rem;color:var(--mut);
white-space:nowrap}
.lk .arrow .cols{font-family:var(--mono);font-size:.66rem;color:var(--navy);
text-align:center;line-height:1.35;white-space:nowrap;overflow:hidden;
text-overflow:ellipsis;max-width:100%}
.lk .why{flex:1 1 0;padding:.6rem .8rem;font-size:.8rem;color:var(--mut);
display:flex;align-items:center;min-width:8em}
@media(max-width:760px){
  .lk{flex-wrap:wrap}
  .lk .node{flex:1 1 45%}
  .lk .arrow{flex:1 1 100%;border-left:none;border-right:none;
  border-top:1px solid var(--hair);border-bottom:1px solid var(--hair)}
  .lk .why{flex:1 1 100%;border-left:none;border-top:1px solid var(--hair)}
}
.lk .nec{font-family:var(--mono);font-size:.62rem;border:1px solid var(--navy);
color:var(--navy);padding:.05rem .3rem;margin-left:.3rem;vertical-align:middle}
"""


def render_links(links: list, stats_links: dict) -> str:
    """关系区 —— 让「对象清单」变成能走通的本体。"""
    keep = [l for l in links if l.verdict == "keep"]
    drop = [l for l in links if l.verdict != "keep"]

    out = [f"<style>{_LINK_CSS}</style>"]
    out.append("<h2>这些对象之间怎么连</h2>")
    out.append(
        "<p class=\"sub\">上一节是「有哪些东西」，这一节是「东西之间怎么走」。"
        "<strong>Agent 要回答「哪些客户额度快用完了」，必须能沿着这些线走一遍。</strong></p>"
    )

    if not links:
        out.append(
            "<div class=\"note\"><span class=\"lbl\">本轮没有产出关系</span>"
            "<p>没有对象间关系时，这仍是一份<strong>对象清单</strong>，不是本体 —— "
            "Agent 走不过去。加 <code>--synthesize</code> 让模型补齐。</p></div>"
        )
        return "".join(out)

    out.append("<div class=\"stats\">")
    out.append(f"<div class=\"stat ok\"><div class=\"n\">{len(keep)}</div>"
               f"<div class=\"l\">经代码验过的关系</div></div>")
    out.append(f"<div class=\"stat {'bad' if drop else ''}\"><div class=\"n\">{len(drop)}</div>"
               f"<div class=\"l\">连接列对不上</div></div>")
    out.append(f"<div class=\"stat\"><div class=\"n\">"
               f"{sum(1 for l in keep if l.necessary_for_decision)}</div>"
               f"<div class=\"l\">回答本轮决策必需</div></div>")
    out.append("</div>")

    out.append("<h3>可用于回答本轮决策的关系</h3>")
    nec = [l for l in keep if l.necessary_for_decision]
    rest = [l for l in keep if not l.necessary_for_decision]
    if not nec:
        out.append('<p class="dim">模型没有标记任何「必需」关系 —— '
                   '下面按基数列出全部，请自行判断哪些用得上。</p>')
        nec, rest = rest, []

    for group, title in ((nec, None), (rest, "其余关系")):
        if not group:
            continue
        if title:
            out.append(f"<h3>{title}</h3>")
        for l in group:
            badge = '<span class="nec">决策必需</span>' if l.necessary_for_decision else ""
            out.append(
                f'<div class="lk"><div class="node"><b>{_e(l.from_name)}</b>'
                f'<span>via {_e(l.from_column)}</span></div>'
                f'<div class="arrow"><span class="sem">{_e(l.semantic_cn)}</span>'
                f'<span class="card2">{_e(l.cardinality)}</span>'
                f'<span class="cols">{_e(l.from_column)} → {_e(l.to_column)}</span></div>'
                f'<div class="node to"><b>{_e(l.to_name)}{badge}</b>'
                f'<span>via {_e(l.to_column)}</span></div>'
                f'<div class="why">{_e(l.reason)}</div></div>'
            )

    if drop:
        out.append("<h3>连接列对不上的关系（机器验不了，交给你判断）</h3>")
        out.append(
            "<p class=\"sub\">这些关系模型提了，但给出的连接列在属性清单里找不到。"
            "<strong>可能是它列错了列名，也可能是这关系靠隐含约定连的（机器无从验证）—— "
            "请人工判断，不要默认丢弃。</strong></p>"
        )
        for l in drop:
            out.append(
                f'<div class="lk drop"><div class="node"><b>{_e(l.from_name)}</b>'
                f'<span>{_e(l.from_column or "未给列")}</span></div>'
                f'<div class="arrow"><span class="sem">？</span></div>'
                f'<div class="node to"><b>{_e(l.to_name)}</b>'
                f'<span>{_e(l.to_column or "未给列")}</span></div>'
                f'<div class="why" style="color:var(--bad)">{_e(l.drop_reason)}</div></div>'
            )
    return "".join(out)
