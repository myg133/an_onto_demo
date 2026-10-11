"""抽取可信度 spike —— 核心逻辑（纯函数，零外部依赖）

本模块只做四件事，全部无 LLM 参与：
  1. 表示 schema（表 / 列 / 类型 / 可空 / 主键 / 注释）
  2. 采样 + 脱敏
  3. 按表分组
  4. **证据验证**（阶段 2）—— 这是本 spike 的心脏

设计规格 §7.1 明确：阶段 1（分组抽取）可外置给 agent，
**阶段 2（证据验证）不可外包** —— 这一条由本模块承载。

对应 REQ-001 / AC-002 / AC-003。
"""

from __future__ import annotations

import hashlib
import json
import re
from collections import Counter
from dataclasses import asdict, dataclass, field
from typing import Any, Iterable, Sequence

# ──────────────────────────────────────────────────────────
# 1 · Schema 表示
# ──────────────────────────────────────────────────────────


@dataclass(frozen=True)
class Column:
    name: str
    data_type: str
    nullable: bool = True
    is_primary_key: bool = False
    comment: str = ""

    @property
    def is_sensitive(self) -> bool:
        """列名看起来是否涉及敏感信息。

        两条刻意的收窄，避免「宁可错杀」毁掉抽取质量：

        1. **不匹配裸 `id`** —— dev_id / order_id 是系统标识，不是 PII；
           真正要脱敏的是 id_card / id_no 这类证件号。
        2. **不匹配所有 `*_name`** —— workshop_name / product_name 不是 PII；
           人员姓名才需要。限定在带「人」前缀的列上。
        """
        n = self.name.lower()
        patterns = (
            # 证件类。注意 *_no 不整体匹配 —— order_no / ticket_no 是业务单号不是 PII
            r"(^|_)(id_?card|id_?no|id_?num|customer_?no|cust_?no|cert_?no"
            r"|identity|passport|ssn)($|_)",
            # 银行/证件掩码字段
            r"(^|_)(bank_?card|card_?no)($|_)",
            r"(^|_)(phone|mobile|tel|email|mail)($|_)",
            r"(^|_)(password|passwd|pwd|secret|token)($|_)",
            r"(^|_)(address|addr|street|postcode|zip)($|_)",
            r"(^|_)(bank|card|account|acct|iban)($|_)",
            # 人员姓名：只认「人味」前缀，不认 workshop_name / product_name
            r"(^|_)(manager|owner|contact|creator|operator|employee|staff|"
            r"user|author|approver|handler|assignee)(_?name)?($|_)",
            r"(^|_)(fullname|realname|nickname|username)($|_)",
        )
        return any(re.search(p, n) for p in patterns)

    @property
    def is_low_cardinality(self) -> bool:
        """枚举型列（状态、类型、等级）—— 语义信息量高，脱敏时应保留。"""
        t = self.data_type.lower()
        if any(k in t for k in ("bool", "enum", "char(1)", "smallint", "tinyint")):
            return True
        return self.name.lower() in _STATUS_LIKE


_STATUS_LIKE = {
    "status", "state", "type", "kind", "category", "level", "grade",
    "flag", "is_deleted", "enabled", "disabled", "active",
}


@dataclass(frozen=True)
class Table:
    name: str
    columns: tuple[Column, ...]
    comment: str = ""
    sample_rows: tuple[tuple[Any, ...], ...] = field(default=())

    @property
    def column_names(self) -> list[str]:
        return [c.name for c in self.columns]

    @property
    def primary_key(self) -> list[str]:
        return [c.name for c in self.columns if c.is_primary_key]

    @property
    def column_map(self) -> dict[str, Column]:
        return {c.name: c for c in self.columns}


@dataclass
class Schema:
    tables: tuple[Table, ...] = field(default_factory=tuple)
    dialect: str = "unknown"

    def table(self, name: str) -> Table | None:
        low = name.lower()
        for t in self.tables:
            if t.name.lower() == low:
                return t
        return None

    def has_column(self, table: str, column: str) -> bool:
        """阶段 2 的核心判定：这张表里到底有没有这一列。"""
        t = self.table(table)
        return bool(t and column.lower() in t.column_map)

    def foreign_keys(self, table_name: str) -> dict[str, str]:
        """启发式推断疑似外键：{本表列名: 目标表名}。

        真实实现应读 information_schema；spike 里用命名启发式够用 ——
        判断「这张表像不像设备表」不需要精确的外键元数据。
        """
        t = self.table(table_name)
        if not t:
            return {}
        hints: dict[str, str] = {}
        for c in t.columns:
            if c.is_primary_key:
                continue
            col = c.name.lower()
            for other in self.tables:
                if other.name == t.name or not other.primary_key:
                    continue
                pk = other.primary_key[0].lower()
                if col == pk or col == f"{pk}_id" or col.endswith(f"_{pk}"):
                    hints[c.name] = other.name
                    break
        return hints

    def to_dict(self) -> dict:
        return {
            "dialect": self.dialect,
            "tables": [
                {
                    "name": t.name,
                    "comment": t.comment,
                    "columns": [
                        {**asdict(c), "is_sensitive": c.is_sensitive}
                        for c in t.columns
                    ],
                    "sample_rows": len(t.sample_rows),
                }
                for t in self.tables
            ],
        }

    @staticmethod
    def from_dict(d: dict) -> "Schema":
        tables = []
        for td in d["tables"]:
            cols = tuple(
                Column(
                    name=c["name"],
                    data_type=c.get("data_type", "text"),
                    nullable=c.get("nullable", True),
                    is_primary_key=c.get("is_primary_key", False),
                    comment=c.get("comment", ""),
                )
                for c in td["columns"]
            )
            tables.append(
                Table(
                    name=td["name"],
                    columns=cols,
                    comment=td.get("comment", ""),
                    sample_rows=tuple(
                        tuple(r) for r in td.get("sample_data", [])  # noqa
                    ),
                )
            )
        return Schema(tables=tuple(tables), dialect=d.get("dialect", "unknown"))


# ──────────────────────────────────────────────────────────
# 2 · 采样与脱敏
# ──────────────────────────────────────────────────────────


def _hashable(v: Any) -> Any:
    """把数据库返回值压成**可哈希**且**可读**的形式。

    真实库里到处是 list / dict / bytes / Decimal / date：
      - psycopg 把 PG 数组（`text[]`）和 `json` / `jsonb` 读成 Python list / dict
      - `bytea` 读成 bytes，str() 出来是 `b'\\x89PNG...'`
    这些直接进 set 会抛 `unhashable type: 'list'`（真实故障，见 REQ-001 运行记录），
    直接 str() 又会吐出没法看的东西。
    """
    if v is None or isinstance(v, (str, int, float, bool)):
        return v
    if isinstance(v, (bytes, bytearray, memoryview)):
        return f"«二进制 {len(bytes(v))} 字节»"
    if isinstance(v, (list, tuple, set)):
        return "[" + ", ".join(str(_hashable(x)) for x in list(v)[:8]) + (
            "…]" if len(v) > 8 else "]"
        )
    if isinstance(v, dict):
        keys = list(v)[:8]
        return "{" + ", ".join(f"{k}: {_hashable(v[k])}" for k in keys) + (
            "…}" if len(v) > 8 else "}"
        )
    from decimal import Decimal

    if isinstance(v, Decimal):
        return float(v)
    return str(v)


def _clip(v: Any, limit: int = 40) -> str:
    """压成可读字符串并截断。"""
    s = str(_hashable(v))
    return s if len(s) <= limit else s[:limit] + "…"


def desensitize(table: Table, rows: Sequence[Sequence[Any]]) -> list[dict]:
    """采样行脱敏。

    策略（按列分别决定，不是一刀切）：
      - 敏感列（手机号/证件/姓名…）→ 只保留类型，值替换为 `«已脱敏»`
      - 枚举型 / 低基数列 → **原样保留**（状态值是语义信息量最高的部分）
      - 其余 → 规范化后截断到 40 字符
    """
    out: list[dict] = []
    low = {
        c.name
        for c in table.columns
        if not c.is_sensitive and (c.is_low_cardinality or _distinct_is_low(table, c, rows))
    }
    for row in rows:
        rec: dict[str, Any] = {}
        for col, val in zip(table.columns, row):
            if col.is_sensitive:
                rec[col.name] = f"«已脱敏:{col.data_type}»"
            elif col.name in low:
                rec[col.name] = _hashable(val)
            else:
                rec[col.name] = _clip(val)
        out.append(rec)
    return out


def _distinct_is_low(table: Table, col: Column, rows: Sequence[Sequence[Any]]) -> bool:
    """这一列的取值是否集中（枚举/状态类）—— 集中则保留原值，信息量最高。"""
    if not rows:
        return False
    idx = table.column_names.index(col.name)
    try:
        vals = {_hashable(r[idx]) for r in rows}
    except (TypeError, ValueError):
        return False
    return 1 < len(vals) <= max(2, len(rows) // 2)


def sample_hash(rows: Iterable[Sequence[Any]]) -> str:
    """样本指纹 —— 用于确认两轮抽取读到的是同一批数据。"""
    blob = json.dumps([list(r) for r in rows], default=str, sort_keys=True)
    return hashlib.sha256(blob.encode()).hexdigest()[:12]


# ──────────────────────────────────────────────────────────
# 3 · 分组
# ──────────────────────────────────────────────────────────


def group_by_table(schema: Schema) -> list[Table]:
    """按表分组。

    设计规格 §7.1 设计点②：表边界是客观的，不需要 LLM 判断；
    跨表归并留给阶段 3 的跨组合成。
    """
    return list(schema.tables)


# ──────────────────────────────────────────────────────────
# 4 · 证据验证（阶段 2）★ 本 spike 的心脏
# ──────────────────────────────────────────────────────────


@dataclass
class Evidence:
    """一条「属性 ← 源列」的声明。

    这是 agent 声称的依据，**不因为它被声称就成立**。
    """

    table: str
    column: str
    claim: str = ""


@dataclass
class Candidate:
    """一个候选本体类型（agent 产出，未经审核）。"""

    key: str                      # 稳定标识，跨轮不变（§7.1 设计点④）
    display_name: str
    source_table: str
    aliases: list[str] = field(default_factory=list)
    properties: list[dict] = field(default_factory=list)  # {name, evidence, nullable}
    evidence: list[Evidence] = field(default_factory=list)
    parent: str | None = None
    from_round: int = 1
    from_agent: str = "unknown"


@dataclass
class VerificationResult:
    kept: list[Candidate] = field(default_factory=list)
    dropped: list[tuple[Candidate, str]] = field(default_factory=list)  # (候选, 丢弃原因)
    checks: list[dict] = field(default_factory=list)

    @property
    def total(self) -> int:
        return len(self.kept) + len(self.dropped)

    @property
    def drop_rate(self) -> float:
        return len(self.dropped) / self.total if self.total else 0.0


def verify_candidates(schema: Schema, candidates: Sequence[Candidate]) -> VerificationResult:
    """阶段 2：逐条核对证据。**这是不可外包的部分**（§7.1）。

    丢弃原因分类（用于判读幻觉类型）：
      - `table_not_found`      agent 编了个不存在的表
      - `column_not_found`     agent 编了个不存在的列
      - `no_evidence`          声称类型却没给任何来源
      - `duplicate_key`        与已有 key 重复（应是增量而非重复提议）
    """
    result = VerificationResult()
    seen_keys: set[str] = set()

    for cand in candidates:
        reasons: list[str] = []

        if not cand.evidence:
            reasons.append("no_evidence")
        for ev in cand.evidence:
            if not schema.table(ev.table):
                reasons.append(f"table_not_found:{ev.table}")
            elif not schema.has_column(ev.table, ev.column):
                reasons.append(f"column_not_found:{ev.table}.{ev.column}")

        if cand.key in seen_keys:
            reasons.append(f"duplicate_key:{cand.key}")

        result.checks.append(
            {
                "key": cand.key,
                "display_name": cand.display_name,
                "source_table": cand.source_table,
                "evidence_count": len(cand.evidence),
                "verdict": "drop" if reasons else "keep",
                "reasons": reasons,
            }
        )

        if reasons:
            result.dropped.append((cand, "; ".join(reasons)))
        else:
            result.kept.append(cand)
            seen_keys.add(cand.key)

    return result


def evidence_stats(result: VerificationResult) -> dict:
    """幻觉率统计 —— REQ-001 的 AC-003 判据。"""
    by_reason: Counter[str] = Counter()
    for _, reason in result.dropped:
        for r in reason.split("; "):
            by_reason[r.split(":")[0]] += 1
    return {
        "total": result.total,
        "kept": len(result.kept),
        "dropped": len(result.dropped),
        "drop_rate": round(result.drop_rate, 3),
        "by_reason": dict(by_reason),
    }
