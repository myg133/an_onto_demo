"""阶段 3 · 跨组合成 —— 把「逐表候选」变成「业务概念」

> **2026-09-30 新增。** 第一次跑真实库时暴露：这个阶段缺失，
> 导致输出是 **DB 翻译**（67% 的候选 key 与表名同名），不是本体。

## 为什么要分三档做

从「96 个按表候选」到「N 个业务概念」，绝大部分判断**不需要 LLM**：

| 档 | 手段 | 是否调 LLM | 覆盖了这次结果的什么 |
| --- | --- | --- | --- |
| **3a 排噪** | 表名模式排除系统表/同步临时表 | ❌ | `_volc_dts_pg_ddl_*` |
| **3b 确定性归并** | 相同 key / 备份·归档·快照·报表后缀 | ❌ | `llm_provider_back_20260701`、`licenses_archive`、`rpt_dim_customer` |
| **3c 相似度聚类** | 列名重叠 + **主键值重叠** + 表名相似 | ❌ | 需要真判断的模糊组 |
| **3d 组内判断** | 「合并还是分开？属性怎么聚合？」 | ✅ **仅模糊组** | —— |
| **3e 关系与层级** | 对象之间的关系、父子 | ✅ 一次 | —— |

**关键设计**：3a/3b/3c 全部是**确定性代码**，零 LLM 成本、结果可复现、可审。
只有真正模糊的才交给 LLM —— 这和设计规格 §2.3「数据能告诉你有什么，
不能告诉你这算什么」是同一条线：**备份表就是同一张表，这是数据形态决定的，
不需要人也不需要模型。**
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, field
from typing import Sequence

from core import Candidate, Column, Schema, Table, _hashable

# ──────────────────────────────────────────────────────────
# 3a · 排噪：系统表 / 同步临时表 / 视图
# ──────────────────────────────────────────────────────────

_NOISE_PATTERNS = (
    r"^_+",                      # _ 开头的临时/内部表
    r"^pg_",                     # PG 自身
    r"^sql_", r"^azure_", r"^backup$",
    r"_volc_", r"_dts_",         # 数据同步工具的中间表
    r"^(information_schema|pg_catalog)$",
)
_NOISE_RE = [re.compile(p, re.I) for p in _NOISE_PATTERNS]

# 备份 / 归档 / 快照 —— 这类是「同源的另一份」，不是「另一个概念」
_SNAPSHOT_SUFFIX = re.compile(
    r"(_back|_bak|_backup|_archive|_archived|_old|_hist|_history|_copy|_tmp|_temp"
    r"|_v\d+|_\d{8}|_\d{6}|_\d{4})+$",
    re.I,
)
# 报表 / 维度表 —— 同一概念的另一种物理形态
_REPORT_PREFIX = re.compile(r"^(rpt|report|rep|dw|dwd|dws|ods|dim|view|v)_", re.I)
_REPORT_INFIX = re.compile(r"_(dim|rpt|report|view|flat|wide|detail|detail_?bak)$", re.I)


def is_noise_table(name: str) -> bool:
    """系统表 / 同步临时表 —— 不该进本体。"""
    return any(r.search(name) for r in _NOISE_RE)


def canonical_name(table: str) -> str:
    """去掉备份/归档/快照/报表修饰，得到**基名**。

    `llm_provider_back_20260701` → `llm_provider`
    `rpt_dim_customer`             → `customer`
    `licenses_archive`             → `licenses`
    """
    t = table
    t = _REPORT_PREFIX.sub("", t)
    t = _REPORT_INFIX.sub("", t)
    t = _SNAPSHOT_SUFFIX.sub("", t)
    return t or table


def merge_note(table: str) -> str:
    """说明这张表为什么被判为「基表的另一份」。"""
    why = []
    if _REPORT_PREFIX.match(table) or _REPORT_INFIX.search(table):
        why.append("报表/维度表")
    if _SNAPSHOT_SUFFIX.search(table):
        why.append("备份/归档/快照")
    return "、".join(why) or "同名"


# ──────────────────────────────────────────────────────────
# 3b/3c · 聚类
# ──────────────────────────────────────────────────────────


@dataclass
class Cluster:
    """一组「疑似同一个业务对象」的候选。"""

    primary: Candidate
    members: list[Candidate] = field(default_factory=list)
    signals: list[str] = field(default_factory=list)   # 为什么聚到一起
    ambiguous: bool = False                            # 需要 LLM 判断吗
    verdict: str = "single"                            # single|merge|split|undecided
    reason: str = ""                                   # 模型/人的裁决理由
    relation: str = "none"                             # none|sibling|subtype
    related_to: str = ""                               # 拆开后的兄弟对象名
    judged_by: str = ""
    llm_properties: list[dict] = field(default_factory=list)

    @property
    def tables(self) -> list[str]:
        return [m.source_table for m in self.members]

    @property
    def merged_properties(self) -> list[dict]:
        """把成员的属性聚到一起，按「列名」去重（同一列不重复列）。"""
        seen: dict[str, dict] = {}
        for m in self.members:
            for p in m.properties:
                col = p.get("column") or p.get("name")
                if not col:
                    continue
                cur = seen.get(col)
                # 同一列出现在多张表 → 标出来，这是「跨表共有属性」的强信号
                if cur:
                    cur["also_in"].append(m.source_table)
                else:
                    seen[col] = {
                        "name": p.get("name", col),
                        "column": col,
                        "nullable": p.get("nullable", True),
                        "also_in": [m.source_table],
                    }
        return list(seen.values())


class _UF:
    def __init__(self, n: int):
        self.p = list(range(n))

    def find(self, x: int) -> int:
        while self.p[x] != x:
            self.p[x] = self.p[self.p[x]]
            x = self.p[x]
        return x

    def union(self, a: int, b: int) -> None:
        ra, rb = self.find(a), self.find(b)
        if ra != rb:
            self.p[rb] = ra


def _col_names(t: Table) -> set[str]:
    return {c.name.lower() for c in t.columns}


def _pk_values(t: Table) -> set[str]:
    """主键（或唯一性最强的前两列）的取值集合 —— 用于判断「是不是同一批东西」。"""
    pk = t.primary_key or t.column_names[:2]
    if not pk or not t.sample_rows:
        return set()
    idxs = [t.column_names.index(c) for c in pk if c in t.column_map]
    if not idxs:
        return set()
    out: set[str] = set()
    for row in t.sample_rows:
        key = "|".join(str(_hashable(row[i])) for i in idxs)
        if not key or key.strip("|"):
            out.add(key)
    return out


def _tokens(name: str) -> set[str]:
    parts = re.split(r"[_\-.\s]+", name.lower())
    return {p for p in parts if len(p) > 2}


def _jaccard(a: set, b: set) -> float:
    if not a or not b:
        return 0.0
    return len(a & b) / len(a | b)


def idf_weights(tables: dict[str, Table]) -> dict[str, float]:
    """列名 → 权重（IDF）。

    为什么不用硬编码 stoplist：`id` / `created_at` / `status` 这些样板列
    **每个客户库都不一样**（有的叫 gmt_create，有的有软删除，有的不分表）。
    实测某真实库：id 出现 93/97 张表、created_at 85/97、status 54/97 ——
    它们把 Jaccard 全都稀释到 30%+，并查集一传递，96 张表会连成一大坨。

    IDF 让「到处都有的列」权重趋近 0，只有**稀有列**（如 `license_id`、`fw_ver`）
    才有区分度。这对任何客户库都通用。
    """
    import math

    n = len(tables) or 1
    freq: dict[str, int] = {}
    for t in tables.values():
        for c in {c.name.lower() for c in t.columns}:
            freq[c] = freq.get(c, 0) + 1
    return {c: math.log(n / k) for c, k in freq.items()}


def _weighted_jaccard(a: set, b: set, w: dict[str, float]) -> float:
    """IDF 加权 Jaccard：交集的权重和 / 并集的权重和。"""
    if not a or not b:
        return 0.0
    inter = sum(w.get(c, 1.0) for c in (a & b))
    union = sum(w.get(c, 1.0) for c in (a | b))
    return inter / union if union else 0.0


def cluster_candidates(
    schema: Schema,
    candidates: Sequence[Candidate],
    col_threshold: float = 0.34,
    pk_threshold: float = 0.30,
) -> tuple[list[Cluster], list[Candidate]]:
    """把候选聚成「疑似同一业务对象」的簇。

    返回 (簇列表, 被排噪的候选)。簇内 `primary` 是该簇的代表候选。
    """
    noise = [c for c in candidates if is_noise_table(c.source_table)]
    live = [c for c in candidates if not is_noise_table(c.source_table)]

    # 簇的成员 = 同 key 的全部候选 + 同表（正常情况 1:1）
    by_key: dict[str, list[Candidate]] = {}
    for c in live:
        by_key.setdefault(c.key, []).append(c)

    # 3b 确定性归并：同 key → 同簇
    clusters: list[Cluster] = []
    assigned: set[int] = set()
    for key, group in by_key.items():
        if len(group) > 1:
            members = sorted(group, key=lambda c: canonical_name(c.source_table) == c.source_table)
            sigs = [f"多张表提出了同一个 key「{key}」"]
            clusters.append(Cluster(primary=members[0], members=members, signals=sigs))

    # 剩余候选按并查集做 3c 相似度聚类
    rest = [c for i, c in enumerate(live) if id(c) not in {id(m) for cl in clusters for m in cl.members}]
    uf = _UF(len(rest))
    tables = {c.source_table: schema.table(c.source_table) for c in rest}
    cols = {n: _col_names(t) for n, t in tables.items() if t}
    pks = {n: _pk_values(t) for n, t in tables.items() if t}
    canon = {n: canonical_name(n) for n in tables}
    W = idf_weights(tables)          # ← IDF 加权，稀释样板列

    def cj(a: str, b: str) -> float:
        return _weighted_jaccard(cols.get(a, set()), cols.get(b, set()), W)

    for i in range(len(rest)):
        for j in range(i + 1, len(rest)):
            a, b = rest[i], rest[j]
            ta, tb = a.source_table, b.source_table
            why = []
            if canon.get(ta) and canon[ta] == canon.get(tb):
                uf.union(i, j)
                why.append(f"基表名相同（{canon[ta]}）")
                continue
            c_v = cj(ta, tb)
            po = _jaccard(pks.get(ta, set()), pks.get(tb, set()))
            if c_v >= col_threshold:
                uf.union(i, j)
                why.append(f"列名 IDF 重叠 {c_v:.0%}")
            if po >= pk_threshold:
                uf.union(i, j)
                why.append(f"主键值重叠 {po:.0%}")

    groups: dict[int, list[Candidate]] = {}
    for i, c in enumerate(rest):
        groups.setdefault(uf.find(i), []).append(c)
    for members in groups.values():
        signals: list[str] = []
        for i in range(len(members)):
            for j in range(i + 1, len(members)):
                ta, tb = members[i].source_table, members[j].source_table
                if canon.get(ta) and canon[ta] == canon.get(tb):
                    signals.append(f"{ta} ↔ {tb}：基表名相同")
                c_v = cj(ta, tb)
                po = _jaccard(pks.get(ta, set()), pks.get(tb, set()))
                if c_v >= col_threshold:
                    signals.append(f"{ta} ↔ {tb}：列名 IDF 重叠 {c_v:.0%}")
                if po >= pk_threshold:
                    signals.append(f"{ta} ↔ {tb}：主键值重叠 {po:.0%}")
        clusters.append(
            Cluster(
                primary=members[0],
                members=members,
                signals=sorted(set(signals)),
                ambiguous=len(members) > 1,   # 合并与否仍需 LLM 拍板
            )
        )

    clusters.sort(key=lambda c: (-len(c.members), c.primary.key))
    return clusters, noise


# ──────────────────────────────────────────────────────────
# 3d · 组内合成判断（LLM 拍板）—— 把判定结果落回簇
# ──────────────────────────────────────────────────────────


@dataclass
class Decision:
    """对一个待判簇的裁决。"""

    tables: list[str]
    verdict: str                 # merge | separate
    reason: str = ""
    relation: str = "none"       # none | sibling | subtype
    parent: str = ""
    child: str = ""
    merged_key: str = ""
    merged_name: str = ""
    merged_properties: list[dict] = field(default_factory=list)
    judged_by: str = ""
    parse_failed: bool = False


def parse_decisions(text: str) -> list[Decision]:
    raw = (text or "").strip()
    if "```" in raw:
        b = re.findall(r"```(?:json)?\s*(.+?)```", raw, re.S)
        raw = b[0] if b else raw
    a, z = raw.find("{"), raw.rfind("}")
    if a < 0 or z <= a:
        return []
    try:
        data = json.loads(raw[a : z + 1])
    except json.JSONDecodeError:
        return []
    out: list[Decision] = []
    for d in data.get("decisions", []):
        out.append(
            Decision(
                tables=[str(t).strip() for t in d.get("tables", [])],
                verdict=(d.get("verdict") or "").lower(),
                reason=d.get("reason", ""),
                relation=(d.get("relation") or "none").lower(),
                parent=d.get("parent", ""),
                child=d.get("child", ""),
                merged_key=d.get("merged_key", ""),
                merged_name=d.get("merged_name", ""),
                merged_properties=d.get("merged_properties", []) or [],
            )
        )
    return out


def apply_decisions(clusters: list[Cluster], decisions: list[Decision]) -> list[Cluster]:
    """把裁决落回簇：merge 保留为一个对象，separate 拆开并记关系。

    **未裁决的簇一律原样保留** —— 宁可让人多看一眼，也不替模型默认合并。
    """
    by_tables: dict[frozenset, Decision] = {}
    for d in decisions:
        if d.tables:
            by_tables[frozenset(d.tables)] = d

    out: list[Cluster] = []
    for c in clusters:
        if len(c.members) == 1:
            c.verdict = "single"
            out.append(c)
            continue
        d = by_tables.get(frozenset(c.tables))
        if d is None:
            c.verdict = "undecided"          # ← 保持原样，交给人
            out.append(c)
            continue
        if d.verdict == "merge":
            c.verdict = "merge"
            c.reason = d.reason
            c.judged_by = d.judged_by or "llm"
            if d.merged_key:
                c.primary.key = d.merged_key
            if d.merged_name:
                c.primary.display_name = d.merged_name
            if d.merged_properties:
                c.llm_properties = d.merged_properties
        else:
            # 拆开：每个成员独立成对象，标注它们彼此的关系
            for m in c.members:
                sub = Cluster(primary=m, members=[m],
                              signals=[f"原判定为同源（{'；'.join(c.signals[:2])}）"],
                              ambiguous=False)
                sub.verdict = "split"
                sub.reason = d.reason
                sub.relation = d.relation
                out.append(sub)
            if d.relation == "sibling" and len(c.members) == 2:
                out[-2].related_to = out[-1].primary.display_name
                out[-1].related_to = out[-2].primary.display_name
            continue
        out.append(c)
    out.sort(key=lambda x: (-len(x.members), x.primary.key))
    return out


# ──────────────────────────────────────────────────────────
# 3e · LinkType —— 对象之间的关系（带证据，带基数，带语义）
# ──────────────────────────────────────────────────────────

_VALID_SEMANTIC = {
    "has": "拥有", "belongs_to": "属于", "depends_on": "依赖",
    "references": "引用", "records": "记录", "derived_from": "由…算出",
}
_VALID_CARD = {"1:1", "1:N", "N:M"}


@dataclass
class Link:
    """一条对象间的关系。对应设计规格 §4.6 的 LinkType。"""

    key: str
    from_key: str
    to_key: str
    from_column: str = ""
    to_column: str = ""
    cardinality: str = "1:N"
    semantic: str = "has"
    reason: str = ""
    necessary_for_decision: bool = False
    from_name: str = ""
    to_name: str = ""
    verdict: str = "pending"     # keep | drop
    drop_reason: str = ""

    @property
    def semantic_cn(self) -> str:
        return _VALID_SEMANTIC.get(self.semantic, self.semantic)

    @property
    def label(self) -> str:
        return f"{self.from_name} —{self.semantic_cn}→ {self.to_name}"


def parse_links(text: str, name_by_key: dict[str, str]) -> list[Link]:
    raw = (text or "").strip()
    if "```" in raw:
        b = re.findall(r"```(?:json)?\s*(.+?)```", raw, re.S)
        raw = b[0] if b else raw
    a, z = raw.find("{"), raw.rfind("}")
    if a < 0 or z <= a:
        return []
    try:
        data = json.loads(raw[a : z + 1])
    except json.JSONDecodeError:
        return []

    out: list[Link] = []
    seen: set[str] = set()
    for d in data.get("links", []):
        fk, tk = str(d.get("from", "")).strip(), str(d.get("to", "")).strip()
        if not fk or not tk or fk == tk:
            continue
        key = str(d.get("key") or f"{fk}_to_{tk}").strip()
        if key in seen:
            continue
        seen.add(key)
        sem = str(d.get("semantic", "has")).strip().lower()
        card = str(d.get("cardinality", "1:N")).strip().upper().replace("：", ":")
        out.append(
            Link(
                key=key, from_key=fk, to_key=tk,
                from_column=str(d.get("from_column", "")).strip(),
                to_column=str(d.get("to_column", "")).strip(),
                cardinality=card if card in _VALID_CARD else "1:N",
                semantic=sem if sem in _VALID_SEMANTIC else "has",
                reason=d.get("reason", ""),
                necessary_for_decision=bool(d.get("necessary_for_decision")),
                from_name=name_by_key.get(fk, fk),
                to_name=name_by_key.get(tk, tk),
            )
        )
    return out


def verify_links(clusters: list[Cluster], links: list[Link], schema: Schema) -> list[Link]:
    """**关系也必须过证据验证** —— 和属性一个标准。

    一条关系成立的前提：
      1. 两端的对象都存在
      2. `from_column` 在起点对象的属性里
      3. `to_column` 在终点对象的属性里
    任何一条不满足 → 丢弃。模型说「它们有关系」不算数，**列能对上才算**。
    """
    props_by_key: dict[str, set[str]] = {}
    tbl_by_key: dict[str, str] = {}
    for c in clusters:
        p = c.merged_properties
        props_by_key[c.primary.key] = {str(x.get("name") or x.get("column")) for x in p} | {
            str(x.get("column")) for x in p
        }
        tbl_by_key[c.primary.key] = c.tables[0]

    for l in links:
        if l.from_key not in props_by_key or l.to_key not in props_by_key:
            l.verdict, l.drop_reason = "drop", f"端点不存在（{l.from_key} / {l.to_key}）"
            continue
        if not l.from_column or l.from_column not in props_by_key[l.from_key]:
            l.verdict = "drop"
            l.drop_reason = f"起点列 {l.from_column or '（未给）'} 不在 {l.from_name} 的属性里"
            continue
        if not l.to_column or l.to_column not in props_by_key[l.to_key]:
            l.verdict = "drop"
            l.drop_reason = f"终点列 {l.to_column or '（未给）'} 不在 {l.to_name} 的属性里"
            continue
        l.verdict = "keep"
    return links


def dangling_links(links: list[Link], clusters: list[Cluster]) -> list[tuple[Link, str]]:
    """既没有连接列、又必须存在的「悬空关系」—— 提出来给人看，不静默丢。

    返回 (关系, 说明)，只包含两端都存在但**没有共同列**的情况：
    这类关系业务上可能真实（靠隐含约定连的），但**机器验不了**，
    所以必须显式交给人判断，而不是被当成幻觉丢掉。
    """
    keys = {c.primary.key for c in clusters}
    out = []
    for l in links:
        if l.verdict == "drop" and l.from_key in keys and l.to_key in keys:
            out.append((l, l.drop_reason))
    return out
