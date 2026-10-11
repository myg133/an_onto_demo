"""抽取可信度 spike —— CLI 主入口

用法
----
  # 零依赖自测（用内置的规范例子库）
  python run.py --self-test

  # 读真库（PG）
  python run.py --dsn "postgresql://user:pw@host:5432/db" --extractor mock

  # 读真库 + 直连模型
  export ANTHROPIC_API_KEY=...
  python run.py --dsn "postgresql://..." --extractor anthropic

  # 读真库 + 外部 A2A agent
  python run.py --dsn "postgresql://..." --extractor a2a \
      --a2a-endpoint https://agent.example.com/rpc --a2a-agent vendor-extractor

输出
----
  out/schema.json     读到的 schema（脱敏后的样本）
  out/report.html     候选本体一页纸 ← 拿给客户看的那份
  out/verification.json 幻觉率对照表（机器可读）
  out/spike-summary.md 人工判读记录模板

对应 REQ-001 的 AC-001 / AC-002 / AC-003 / AC-005。
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from core import (  # noqa: E402
    Schema,
    Table,
    evidence_stats,
    desensitize,
    group_by_table,
    sample_hash,
    verify_candidates,
)
from extractors import (  # noqa: E402
    OPENAI_COMPATIBLE_PRESETS,
    A2AExtractor,
    AnthropicExtractor,
    MockExtractor,
    OpenAICompatibleExtractor,
)
from render import render_onepager, render_concepts  # noqa: E402
from extractors import build_link_prompt, build_synth_prompt  # noqa: E402
from synthesize import (  # noqa: E402
    apply_decisions,
    cluster_candidates,
    dangling_links,
    parse_decisions,
    parse_links,
    verify_links,
)
from fixture import sample_schema  # noqa: E402


# ──────────────────────────────────────────────────────────
# 数据库适配（薄层 —— 核心逻辑不依赖它）
# ──────────────────────────────────────────────────────────


def _db_hint(dialect: str, dsn: str, err: Exception) -> str:
    """把连接失败翻译成人能读的话 —— DSN 写错是最常见的错误。"""
    msg = str(err).strip().splitlines()[0] if str(err).strip() else type(err).__name__
    masked = _mask_dsn(dsn)
    hints = [
        f"✗ 连不上 {dialect}",
        f"  连接串：{masked}",
        f"  原因：{msg}",
        "",
        "  逐项检查：",
        "   1. 网络可达？本机能 ping 通 host / 端口通吗",
        "   2. 账号密码对吗？**务必用只读账号**（本工具禁止写操作）",
        "   3. IP 白名单：数据库是否只允许特定来源 IP 访问",
        "   4. DSN 格式对吗？",
    ]
    if dialect == "MySQL":
        hints.append('       --dsn "host=...;user=ro;password=***;dbname=..."')
    else:
        hints.append('       --dsn "postgresql://user:pw@host:5432/db"')
    return "\n".join(hints)


def _mask_dsn(dsn: str) -> str:
    """遮掉 DSN 里的密码，避免打进日志或截图。"""
    import re as _re

    d = _re.sub(r"(password=)([^;]*)", r"\1***", dsn, flags=_re.I)
    d = _re.sub(r"://([^:/@]+):([^@]+)@", r"://\1:***@", d)
    return d


def _quote(name: str) -> str:
    """表名转义。非 public schema 写成 `schema`.`table`。

    名字来自系统目录，理论上可能含引号 —— 这里把 `"` 翻倍，杜绝拼接注入。
    """
    if "." in name:
        sch, tbl = name.split(".", 1)
        return f'"{sch.replace(chr(34), chr(34) * 2)}"."{tbl.replace(chr(34), chr(34) * 2)}"'
    return f'"{name.replace(chr(34), chr(34) * 2)}"'


# 主键判定：information_schema.key_column_usage 里**没有 is_pk 列**
# （只有 constraint_catalog/schema/name、table_*、column_name、
#   ordinal_position、position_in_unique_constraint）。
# 只能靠 constraint_name = 'PRIMARY KEY' 来识别主键列。
_PG_SCHEMA_SQL = """
SELECT c.table_schema,
       c.table_name,
       c.column_name,
       c.data_type,
       c.is_nullable,
       (k.column_name IS NOT NULL)      AS is_pk,
       COALESCE(pg_col.description, '') AS comment
FROM information_schema.columns c
LEFT JOIN information_schema.key_column_usage k
       ON  k.table_schema    = c.table_schema
       AND k.table_name      = c.table_name
       AND k.column_name     = c.column_name
       AND k.constraint_name = 'PRIMARY KEY'
LEFT JOIN pg_catalog.pg_namespace  ns ON ns.nspname = c.table_schema
LEFT JOIN pg_catalog.pg_class      cl ON cl.relname      = c.table_name
                                    AND cl.relnamespace = ns.oid
LEFT JOIN pg_catalog.pg_attribute  at ON at.attrelid  = cl.oid
                                    AND at.attname   = c.column_name
                                    AND at.attnum    > 0
                                    AND NOT at.attisdropped
LEFT JOIN pg_catalog.pg_description pg_col
                                    ON pg_col.objoid   = cl.oid
                                    AND pg_col.objsubid = at.attnum
WHERE c.table_schema NOT IN ('pg_catalog', 'information_schema')
  AND c.table_schema NOT LIKE 'pg_toast%'
  AND c.table_schema NOT LIKE 'pg_temp_%'
ORDER BY c.table_schema, c.table_name, c.ordinal_position
"""


def read_schema_postgres(dsn: str, sample_n: int = 20) -> Schema:
    try:
        import psycopg
    except ImportError as e:
        raise SystemExit(
            "缺 psycopg。请用：uv run --extra postgres run.py ...\n"
            "  或一次性装全：uv run --extra full run.py ..."
        ) from e

    tables: list[Table] = []
    try:
        conn_ctx = psycopg.connect(
            dsn, options="-c default_transaction_read_only=on"
        )
    except Exception as e:  # noqa: BLE001
        raise SystemExit(_db_hint("PostgreSQL", dsn, e)) from e

    with conn_ctx as conn:
        with conn.cursor() as cur:
            try:
                cur.execute(_PG_SCHEMA_SQL)
            except Exception as e:  # noqa: BLE001
                raise SystemExit(
                    f"✗ 读取 schema 失败\n  原因：{str(e).strip().splitlines()[0]}\n"
                    "  常见原因：账号对 information_schema 权限不足"
                ) from e
            rows = cur.fetchall()
            by_table: dict[str, list] = {}
            for schema, t, col, dt, nullable, is_pk, comment in rows:
                by_table.setdefault(f"{schema}.{t}" if schema != "public" else t, []).append(
                    (col, dt, nullable, is_pk, comment)
                )

            from core import Column

            # 逐表采样 —— 单表失败只影响该表，不中断整体
            for tname, cols in by_table.items():
                samples: tuple = ()
                try:
                    cur.execute(f"SELECT * FROM {_quote(tname)} LIMIT {int(sample_n)}")
                    samples = tuple(tuple(r) for r in cur.fetchall())
                except Exception:  # noqa: BLE001
                    conn.rollback()   # 单表失败：回滚后继续
                tables.append(
                    Table(
                        name=tname,
                        columns=tuple(
                            Column(
                                name=c[0],
                                data_type=c[1],
                                nullable=c[2] == "YES",
                                is_primary_key=bool(c[3]),
                                comment=(c[4] or "").strip(),
                            )
                            for c in cols
                        ),
                        sample_rows=tuple(tuple(r) for r in samples),
                    )
                )
    return Schema(tables=tuple(tables), dialect="postgresql")


def read_schema_mysql(dsn: str, sample_n: int = 20) -> Schema:
    """MySQL。dsn 接受 libpq 风格的 key=value 串：

        --dsn "host=db.internal;user=ro;password=***;dbname=crm;charset=utf8mb4"
    """
    try:
        import pymysql
    except ImportError as e:
        raise SystemExit(
            "缺 pymysql。请用：uv run --extra mysql run.py ...\n"
            "  或一次性装全：uv run --extra full run.py ..."
        ) from e

    params: dict = {}
    for part in dsn.split(";"):
        if "=" in part:
            k, v = part.split("=", 1)
            params[k.strip()] = v.strip()
    if "host" not in params or "dbname" not in params:
        raise SystemExit(
            "MySQL DSN 需要 host 与 dbname，例如：\n"
            '  --dsn "host=db.internal;user=ro;password=***;dbname=crm"'
        )

    from core import Column

    try:
        conn = pymysql.connect(
            connect_timeout=10, read_timeout=60, charset="utf8mb4", **params
        )
    except Exception as e:  # noqa: BLE001
        raise SystemExit(_db_hint("MySQL", dsn, e)) from e
    tables: list[Table] = []
    with conn.cursor() as cur:
        cur.execute(
            """
            SELECT TABLE_NAME, COLUMN_NAME, DATA_TYPE, IS_NULLABLE,
                   COLUMN_KEY, COLUMN_COMMENT
            FROM information_schema.COLUMNS
            WHERE TABLE_SCHEMA = DATABASE()
            ORDER BY TABLE_NAME, ORDINAL_POSITION
            """
        )
        by_table: dict[str, list] = {}
        for t, col, dt, nullable, key, comment in cur.fetchall():
            by_table.setdefault(t, []).append((col, dt, nullable, key, comment))
        for tname, cols in by_table.items():
            try:
                cur.execute(f"`{tname}` LIMIT {int(sample_n)}")
                samples = cur.fetchall()
            except Exception:
                conn.rollback()
                samples = []
            tables.append(
                Table(
                    name=tname,
                    columns=tuple(
                        Column(
                            name=c[0],
                            data_type=c[1],
                            nullable=c[2] == "YES",
                            is_primary_key=(c[3] or "") == "PRI",
                            comment=(c[4] or "").strip(),
                        )
                        for c in cols
                    ),
                    sample_rows=tuple(tuple(r) for r in samples),
                )
            )
    conn.close()
    return Schema(tables=tuple(tables), dialect="mysql")


# ──────────────────────────────────────────────────────────
# 主流程
# ──────────────────────────────────────────────────────────


def run(schema: Schema, extractor, out_dir: Path, project: str, scope: str,
        synthesize: bool = False) -> int:
    out_dir.mkdir(parents=True, exist_ok=True)

    # 阶段 0 · 准备 —— 落盘脱敏后的 schema，供人核对
    schema_doc = {
        "dialect": schema.dialect,
        "tables": [
            {
                "name": t.name,
                "comment": t.comment,
                "primary_key": t.primary_key,
                "columns": [
                    {
                        "name": c.name,
                        "data_type": c.data_type,
                        "nullable": c.nullable,
                        "is_primary_key": c.is_primary_key,
                        "comment": c.comment,
                        "is_sensitive": c.is_sensitive,
                    }
                    for c in t.columns
                ],
                "sample_hash": sample_hash(t.sample_rows),
                "desensitized_sample": desensitize(t, t.sample_rows),
            }
            for t in group_by_table(schema)
        ],
    }
    (out_dir / "schema.json").write_text(
        json.dumps(schema_doc, ensure_ascii=False, indent=2, default=str),
        encoding="utf-8",
    )
    print(f"[阶段 0] schema 已落盘：{out_dir/'schema.json'}"
          f"（{len(schema.tables)} 张表）")

    # 阶段 1 · 分组抽取
    all_candidates = []
    failed_groups: list[str] = []
    for i, table in enumerate(group_by_table(schema), 1):
        try:
            cands = extractor.extract(
                table,
                {
                    "round": 1,
                    "scope": scope,
                    "existing": "（无，这是第一轮）",
                    "desensitized": desensitize(table, table.sample_rows),
                },
            )
            all_candidates.extend(cands)
            print(f"  [{i}/{len(schema.tables)}] {table.name}: {len(cands)} 个候选")
        except Exception as e:  # noqa: BLE001
            # 失败必须显式 —— §7.1 设计点⑤，否则静默失败会伪造收敛
            failed_groups.append(table.name)
            print(f"  [{i}/{len(schema.tables)}] {table.name}: ✗ 抽取失败 —— {e}")

    if failed_groups:
        print(f"\n⚠ {len(failed_groups)} 组抽取失败：{failed_groups}")
        print("  这些组**不参与收敛判定**（§7.1 设计点⑤）")

    # 阶段 2 · 证据验证
    result = verify_candidates(schema, all_candidates)
    stats = evidence_stats(result)
    print(f"\n[阶段 2] 证据验证：保留 {stats['kept']} / {stats['total']}"
          f"，幻觉率 {stats['drop_rate']:.1%}")
    if stats["by_reason"]:
        for k, v in stats["by_reason"].items():
            print(f"         {k}: {v}")

    (out_dir / "verification.json").write_text(
        json.dumps(
            {
                "stats": stats,
                "failed_groups": failed_groups,
                "extractor": {
                    "name": getattr(extractor, "name", "?"),
                    "version": getattr(extractor, "version", "?"),
                },
                "checks": result.checks,
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )

    # ── 阶段 3 · 跨组合成（把「逐表候选」变成「业务概念」）──
    clusters, noise = cluster_candidates(schema, result.kept)
    multi = [c for c in clusters if len(c.members) > 1]
    print(f"\n[阶段 3] 跨组合成：{len(result.kept)} 个逐表候选 → "
          f"**{len(clusters)} 个业务对象**")
    if noise:
        print(f"         排噪 {len(noise)} 张系统/同步临时表："
              f"{[n.source_table for n in noise][:3]}")
    if multi:
        print(f"         {len(multi)} 组待判断是否同一个概念：")
        for c in multi[:6]:
            props = c.merged_properties
            shared = [p['column'] for p in props if len(p['also_in']) > 1]
            print(f"           · {', '.join(c.tables)}")
            print(f"             信号：{c.signals[0] if c.signals else '—'}"
                  f"  |  聚合属性 {len(props)} 个，跨表共有 {len(shared)} 个")

    # ── 阶段 3d · 组内合成判断（LLM 拍板：合并还是分开）──
    if synthesize and multi:
        print(f"\n[阶段 3d] 组内合成判断：{len(multi)} 组待判", flush=True)
        try:
            raw = extractor.complete(build_synth_prompt(multi, scope))
            decisions = parse_decisions(raw)
            if decisions:
                for d in decisions:
                    d.judged_by = getattr(extractor, "name", "llm")
                clusters = apply_decisions(clusters, decisions)
                m = sum(1 for c in clusters if c.verdict == "merge")
                sp = sum(1 for c in clusters if c.verdict == "split")
                print(f"          判定：合并 {m} 组，拆开 {sp} 组")
                for c in clusters:
                    if c.verdict in ("merge", "split") and c.reason:
                        tag = "合并" if c.verdict == "merge" else "拆开"
                        print(f"          [{tag}] {c.primary.display_name}: {c.reason[:60]}")
            else:
                print("          ⚠ 模型返回无法解析 → **不替它做决定**，"
                      "全部保持原样交人工判断")
                (out_dir / "synth-raw.txt").write_text(raw, encoding="utf-8")
        except Exception as e:  # noqa: BLE001
            print(f"          ⚠ 合成判断失败（{e}）→ 保持原样交人工判断")
    elif multi and not synthesize:
        print(f"\n[提示] {len(multi)} 组待判。加 --synthesize 可让模型逐组拍板；"
              f"不加则保持原样交人工判断")

    # ── 阶段 3e · 跨对象关系（LinkType）──
    links = None
    if synthesize and len(clusters) >= 2:
        name_by_key = {c.primary.key: c.primary.display_name for c in clusters}
        print(f"\n[阶段 3e] 跨对象关系：{len(clusters)} 个对象两两判断", flush=True)
        try:
            raw_l = extractor.complete(build_link_prompt(clusters, scope, name_by_key))
            links = verify_links(clusters, parse_links(raw_l, name_by_key), schema)
            kept = sum(1 for l in links if l.verdict == "keep")
            need = sum(1 for l in links if l.necessary_for_decision and l.verdict == "keep")
            print(f"          产出 {len(links)} 条关系，代码验过 {kept} 条"
                  f"（其中 {need} 条为回答本轮决策所必需）")
            for l in links:
                mark = "✓" if l.verdict == "keep" else "✗"
                star = " ★" if l.necessary_for_decision and l.verdict == "keep" else ""
                print(f"          {mark} {l.label:34s} {l.cardinality}{star}")
                if l.drop_reason:
                    print(f"              {l.drop_reason}")
            dang = dangling_links(links, clusters)
            if dang:
                print(f"          ⚠ {len(dang)} 条机器验不了，已在页面上单列给人工判断")
        except Exception as e:  # noqa: BLE001
            print(f"          ⚠ 关系抽取失败（{e}）→ 页面会显示「本轮没有产出关系」")
    elif len(clusters) >= 2 and not synthesize:
        print("\n[提示] 加 --synthesize 会一并补齐对象间关系；"
              "当前只抽了对象，没有线，Agent 走不过去")

    # ── 交付物 1：逐表候选对照（原始抽取质量，可审幻觉率）──
    (out_dir / "report.html").write_text(
        render_onepager(schema, result.kept, result.dropped, stats, project),
        encoding="utf-8",
    )
    # ── 交付物 2：业务概念一页纸（这才是「本体」，给客户判读用）──
    (out_dir / "concepts.html").write_text(
        render_concepts(schema, clusters, stats, project, scope, links),
        encoding="utf-8",
    )
    print(f"\n[交付] report.html   逐表候选对照（审幻觉率用）")
    print(f"       concepts.html 业务概念一页纸（给客户判读用）← AC-005 主交付物")

    # 人工判读记录模板
    (out_dir / "spike-summary.md").write_text(_summary_tpl(schema, stats, failed_groups,
                                                            getattr(extractor, "name", "?")),
                                              encoding="utf-8")
    return 0 if stats["drop_rate"] < 0.30 else 1


def _summary_tpl(schema: Schema, stats: dict, failed: list[str], extractor: str) -> str:
    verdict = (
        "✅ 通过（幻觉率 < 30%）"
        if stats["drop_rate"] < 0.30
        else ("⚠ 勉强（30–50%）" if stats["drop_rate"] < 0.50 else "❌ 不通过（> 50%）")
    )
    return f"""# 抽取可信度 spike · 判读记录

| 项 | 值 |
| --- | --- |
| 生成时间 | 见 `verification.json` |
| 抽取器 | `{extractor}` |
| 扫描表数 | {len(schema.tables)} |
| 候选总数 | {stats['total']} |
| 通过验证 | {stats['kept']} |
| 幻觉率 | **{stats['drop_rate']:.1%}** |
| 抽取失败组 | {len(failed)} 组{f'（{failed}）' if failed else ''} |
| **机器判据（AC-003）** | {verdict} |

---

## AC-002 / AC-003 · 幻觉率对照

见 `verification.json` 的 `checks` 数组，逐条列出「agent 提了什么」与「为什么被留/被丢」。

**人工补充观察**：

-
-
-

## AC-004 · 跨表合成判断

**待人工验证的题目**：`t_dev` / `t_device_info` / `dev_backup`（或实际库中的同类表）
该判为「3 个类型 + 关系」还是「1 个类型」？

| 判断 | 是否正确 | 实际应该是 | 备注 |
| --- | --- | --- | --- |
| （机器的判断） | | | |

## AC-005 · 客户一眼判读 ← 核心判据

把 `report.html` 拿给客户业务人员，问：**「这些是你们说的东西吗？哪里不对？」**

| 判据 | 结果 |
| --- | --- |
| 客户能**具体指出**哪里对、哪里不对 | ☐ 是 ☐ 否 |
| 客户只是说「还行」「差不多」 | ☐ 是 ☐ 否 |
| 客户完全看不懂在说什么 | ☐ 是 ☐ 否 |

> ⚠ **「还行」但指不出具体哪里错 = 不通过。**
> 那说明抽取没有真正降低客户的认知负担 —— 核心赌注未成立。

**客户的具体意见**（逐条记录）：

1.
2.
3.

---

## 决策出口

| spike 结果 | 下一步 |
| --- | --- |
| 候选本体客户能一眼判读 | 启动内核重写（Rust），按设计规格 §0.2 |
| 客户判读不了，但证据链清晰 | 调 prompt / 加领域提示，再跑一轮 |
| 幻觉率 > 50% | **停下**，重新评估「抽取结构」这个核心假设 |
| 跨表合成判断不可靠 | 该能力不外置，自研阶段 3（需回改规格 §7.1） |
"""


def _extractor_choice(v: str) -> str:
    """校验 --extractor，并在用户把 --extra 写错成 --extractor 时给出指路。"""
    valid = {"mock", "anthropic", "openai", "a2a"}
    if v in valid:
        return v
    if v in {"full", "postgres", "mysql"}:
        ap_exit = sys.stderr
        ap_exit.write(
            f"✗ --extractor {v!r} 不是抽取器，而是依赖分组名。\n"
            f"  它是 **uv 的参数**，要写在 run.py 前面：\n\n"
            f"    uv run --extra {v} run.py --extractor anthropic --dsn ...\n\n"
            f"  合法取值：{', '.join(sorted(valid))}\n"
        )
        raise SystemExit(2)
    return v


def main() -> int:
    ap = argparse.ArgumentParser(description="抽取可信度 spike")
    ap.add_argument("--self-test", action="store_true", help="用内置规范例子库跑通全链路")
    ap.add_argument("--dsn", help="数据库连接串")
    ap.add_argument("--dialect", choices=["postgres", "mysql"], default="postgres")
    ap.add_argument("--sample", type=int, default=20, help="每表采样行数")
    ap.add_argument("--extractor", default="mock",
                    type=_extractor_choice,
                    metavar="{mock,anthropic,openai,a2a}",
                    help="mock=不调模型 | anthropic=Claude | openai=任意 OpenAI 兼容端点 | a2a=外部 agent")
    ap.add_argument("--provider", default="openai",
                    choices=sorted(OPENAI_COMPATIBLE_PRESETS),
                    help="OpenAI 兼容端点预设（配合 --extractor openai）")
    ap.add_argument("--base-url", help="自定义 OpenAI 兼容端点，覆盖 --provider 预设")
    ap.add_argument("--api-key", help="直接给 key（优先于环境变量）")
    ap.add_argument("--model", default="claude-sonnet-4-5",
                    help="模型名。--extractor openai 时覆盖预设默认值")
    ap.add_argument("--a2a-endpoint")
    ap.add_argument("--a2a-agent", default="vendor-extractor")
    ap.add_argument("--scope", default="", help="决策清单摘要（限定抽取范围）")
    ap.add_argument("--tables", help="只抽这些表，逗号分隔，支持 * 通配，如 't_dev,*ticket*'。"
                                     "**这是决策清单的范围闸门**（规格 §2.6）—— 全库 97 张表时必用")
    ap.add_argument("--project", default="客户环境")
    ap.add_argument("--out", default="out")
    ap.add_argument("--synthesize", action="store_true",
                    help="阶段3d：把「疑似同源」的簇交给模型逐组拍板（合并还是分开）")
    ap.add_argument("--schema-only", action="store_true",
                    help="只读 schema 并打印，不调抽取器（用于快速验证连通性与目录权限）")
    a = ap.parse_args()

    # 取 schema
    if a.self_test or not a.dsn:
        print("== 自测模式：使用内置规范例子库（零依赖）==\n")
        schema = sample_schema()
    else:
        schema = (
            read_schema_postgres(a.dsn, a.sample)
            if a.dialect == "postgres"
            else read_schema_mysql(a.dsn, a.sample)
        )
        print(f"== 读取 {a.dialect}：{len(schema.tables)} 张表 ==\n")

    if a.tables:
        schema = _filter_tables(schema, a.tables)

    if a.schema_only:
        return _schema_only(schema, a)

    # 取抽取器
    if a.extractor == "anthropic":
        extractor = AnthropicExtractor(model=a.model)
    elif a.extractor == "openai":
        preset = OPENAI_COMPATIBLE_PRESETS[a.provider]
        extractor = OpenAICompatibleExtractor(
            provider=a.provider,
            base_url=a.base_url,
            model=a.model if a.model != "claude-sonnet-4-5" else None,
            api_key=a.api_key,
        )
        print(f"   端点：{extractor.base_url}  模型：{extractor.model}")
    elif a.extractor == "a2a":
        if not a.a2a_endpoint:
            raise SystemExit("A2A 模式需要 --a2a-endpoint")
        extractor = A2AExtractor(endpoint=a.a2a_endpoint, agent_id=a.a2a_agent)
    else:
        if not (a.self_test or not a.dsn):
            print("提示：mock 抽取器不调模型，仅验证链路。"
                  "真实抽取请用 --extractor anthropic 或 a2a\n")
        extractor = MockExtractor(_mock_fixture(schema))

    return run(schema, extractor, Path(a.out), a.project, a.scope, a.synthesize)


def _filter_tables(schema: Schema, pattern: str) -> Schema:
    """按逗号分隔的通配模式筛表 —— 决策清单的范围闸门（规格 §2.6）。

    为什么必须有这个：真实客户库动辄上百张表，全抽一轮 = 上百次模型调用，
    既慢又贵，而且**违背「决策清单限定范围」这条设计**。
    """
    import fnmatch

    pats = [p.strip() for p in pattern.split(",") if p.strip()]
    kept = [t for t in schema.tables
            if any(fnmatch.fnmatch(t.name, p) or fnmatch.fnmatch(t.name.lower(), p.lower())
                   for p in pats)]
    dropped = len(schema.tables) - len(kept)
    print(f"\n[范围闸门] 模式 {pattern!r} → 命中 {len(kept)} 张表"
          f"（按决策清单排除 {dropped} 张）")
    if not kept:
        raise SystemExit(
            f"✗ --tables 没匹配到任何表。现有表名（前 15 个）：\n"
            + "\n".join(f"    {t.name}" for t in schema.tables[:15])
        )
    if dropped and len(kept) / len(schema.tables) > 0.5:
        print(f"  ⚠ 仍抽了 {len(kept)/len(schema.tables):.0%} 的表 —— "
              "范围闸门的作用是收敛，不是全覆盖")
    s2 = Schema(tables=tuple(kept), dialect=schema.dialect)
    return s2


def _schema_only(schema: Schema, a) -> int:
    """只打印 schema —— 用来快速判断「是连不上，还是目录权限不足」。"""
    print(f"\n读到 {len(schema.tables)} 张表：\n")
    for t in schema.tables:
        pk = "、".join(t.primary_key) or "（无主键）"
        print(f"  {t.name}  — {len(t.columns)} 列，主键 {pk}，采样 {len(t.sample_rows)} 行")
        if t.comment:
            print(f"      表注释：{t.comment}")
        if not t.sample_rows:
            print(f"      ⚠ 采不到样本（无 SELECT 权限，或表为空）")
        for c in t.columns:
            flags = []
            if c.is_primary_key: flags.append("PK")
            if not c.nullable: flags.append("NOT NULL")
            if c.is_sensitive: flags.append("敏感")
            print(f"        - {c.name} ({c.data_type}) {' '.join(flags)}"
                  + (f"  // {c.comment}" if c.comment else ""))
    print("\n✓ schema 读取正常。下一步去掉 --schema-only 即可跑抽取。")
    return 0


def _mock_fixture(schema: Schema) -> dict:
    """自测用的预设答案 —— 刻意混入坏证据，验证阶段 2 拦得住。"""
    from core import Candidate, Evidence

    def t(name):
        return schema.table(name)

    fixture: dict[str, list[Candidate]] = {}

    if t("t_dev"):
        fixture["t_dev"] = [
            Candidate(
                key="device", display_name="设备", source_table="t_dev",
                aliases=["机台", "机器"],
                properties=[
                    {"name": "设备编号", "column": "dev_code", "nullable": False},
                    {"name": "型号", "column": "model", "nullable": True},
                    {"name": "状态", "column": "status", "nullable": False},
                ],
                evidence=[
                    Evidence("t_dev", "dev_code", "设备唯一编号"),
                    Evidence("t_dev", "model", "型号"),
                    Evidence("t_dev", "status", "运行状态"),
                ],
            ),
            # ✗ 故意编造不存在的列 —— 应被拦下
            Candidate(
                key="device_bogus", display_name="设备(幻觉)", source_table="t_dev",
                properties=[{"name": "虚拟列", "column": "nonexistent_col", "nullable": True}],
                evidence=[Evidence("t_dev", "nonexistent_col", "编造的列")],
            ),
        ]
    if t("t_ticket"):
        fixture["t_ticket"] = [
            Candidate(
                key="ticket", display_name="工单", source_table="t_ticket",
                properties=[
                    {"name": "工单号", "column": "ticket_id", "nullable": False},
                    {"name": "标题", "column": "title", "nullable": True},
                ],
                evidence=[
                    Evidence("t_ticket", "ticket_id", "工单主键"),
                    Evidence("t_ticket", "title", "工单标题"),
                ],
            ),
        ]
    # ✗ 故意编造不存在的表 —— 应被拦下
    if t("t_device_info"):
        fixture["t_device_info"] = [
            Candidate(
                key="device_info", display_name="设备信息(幻觉)", source_table="t_device_info",
                properties=[{"name": "x", "column": "y", "nullable": True}],
                evidence=[Evidence("t_no_such_table", "some_col", "整张表都是编的")],
            ),
        ]
    return fixture


if __name__ == "__main__":
    raise SystemExit(main())
