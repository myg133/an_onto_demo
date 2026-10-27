#!/usr/bin/env python3
"""账本一致性审计：req-registry.md × demands/*/status.md × develop 提交。

只读，不写任何被跟踪文件。输出 markdown 报告 + 控制台摘要（ASCII，避免编码问题）。
"""
import re
import subprocess
import sys
from pathlib import Path

BA = Path(r"D:\MyCodes\AI\an_onto_demo\BA")
CODE = Path(r"D:\MyCodes\AI\an_onto_demo\code")
REG = BA / "dispatch" / "req-registry.md"
OUT = BA / ".local" / "audit" / "ledger-audit-20261027.md"

DONE_FAMILY = {"已完成", "已验证"}
OPEN_FAMILY = {"草稿", "已评审", "已就绪", "进行中", "待验证", "已退回", "待裁定", "已取消", "已合并", "已关闭"}


def norm_status(cell: str) -> str:
    """从表格单元格里抽状态词：优先 **X**，其次第一个中英文词。"""
    m = re.search(r"\*\*(.+?)\*\*", cell)
    raw = m.group(1) if m else cell
    raw = raw.strip().strip("*").strip()
    # 去掉括号里的补充说明
    raw = re.split(r"[（(]", raw)[0].strip()
    for key in ("已完成", "已验证", "已就绪", "进行中", "待验证", "已退回",
                "已取消", "已关闭", "已合并", "待裁定", "已评审", "草稿"):
        if raw.startswith(key) or key in raw:
            return key
    return raw[:12] if raw else "(空)"


def parse_registry():
    rows = []
    text = REG.read_text(encoding="utf-8")
    for line in text.splitlines():
        if not line.startswith("| REQ-"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 4:
            continue
        req = cells[0]
        rows.append({
            "req": req,
            "name": cells[1],
            "status": norm_status(cells[2]),
            "status_raw": cells[2],
            "who": cells[3] if len(cells) > 3 else "",
            "date": cells[4] if len(cells) > 4 else "",
            "line": len([1]),
        })
    return rows


def status_md(req: str):
    p = BA / "demands" / req / "status.md"
    if not p.exists():
        return None
    txt = p.read_text(encoding="utf-8", errors="replace")
    # 取「## 当前状态」之后的第一处 **...**
    m = re.search(r"##\s*当前状态\s*\n+(.+?)\n", txt)
    seg = m.group(1) if m else txt[:800]
    m2 = re.search(r"\*\*(.+?)\*\*", seg)
    if m2:
        return norm_status(m2.group(1))
    m3 = re.search(r"(已完成|已验证|已就绪|进行中|待验证|已退回|草稿|已取消|已关闭)", seg)
    return m3.group(1) if m3 else "(无法识别)"


def git_facts(req: str):
    try:
        out = subprocess.run(
            ["git", "-C", str(CODE), "log", "--all", "--format=%h\t%ad\t%s",
             "--date=short", f"--grep={req}"],
            capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
    except Exception as e:  # noqa: BLE001
        return {"count": -1, "merge": None, "last": None, "err": str(e)}
    lines = [l for l in out.stdout.splitlines() if l.strip()]
    merge = None
    for l in lines:
        parts = l.split("\t")
        if len(parts) >= 3 and parts[2].startswith("[Merge]"):
            merge = l
            break
    return {"count": len(lines), "merge": merge, "last": lines[0] if lines else None}


def main():
    rows = parse_registry()
    by_req = {}
    for r in rows:
        by_req.setdefault(r["req"], []).append(r)

    dirs = sorted([p.name for p in (BA / "demands").iterdir() if p.is_dir() and p.name.startswith("REQ-")])

    dup, mismatch, missing_status, stale_open, suspicious_done, no_registry = [], [], [], [], [], []
    unverifiable = []
    for req, rs in sorted(by_req.items()):
        if len(rs) > 1:
            dup.append((req, [r["status"] for r in rs]))
        md = status_md(req)
        if md is None:
            missing_status.append(req)
        elif md != rs[-1]["status"]:
            mismatch.append((req, rs[-1]["status"], md))
        facts = git_facts(req)
        merged = facts["merge"] is not None
        if merged and rs[-1]["status"] in OPEN_FAMILY:
            stale_open.append((req, rs[-1]["status"], facts["merge"]))
        if not merged and rs[-1]["status"] in DONE_FAMILY:
            if facts["count"] > 0:
                suspicious_done.append((req, rs[-1]["status"], facts["count"], facts["last"]))
            else:
                unverifiable.append((req, rs[-1]["status"]))

    reg_reqs = set(by_req)
    for d in dirs:
        if d not in reg_reqs:
            no_registry.append(d)

    L = []
    add = L.append
    dirs_missing_md = [d for d in dirs if not (BA / "demands" / d / "status.md").exists()]
    add("# 账本一致性审计 · 2026-10-27（lion 只读复核）\n")
    add("> 产出：`ba-lion-michael-workstation-49056-20261027-094244` · **只读**，未改任何被审计文件。\n"
        "> 复跑：`python BA/docs/reports/evidence/2026-10-27-ledger_audit.py`（脚本与本报告同处 `docs/reports/`）\n")
    add(f"- 登记表行数：**{len(rows)}** · 涉及 REQ：**{len(by_req)}** · `demands/` 目录：**{len(dirs)}**")
    add(f"- 发现：同名重复 **{len(dup)}** · 登记表×status.md 状态不一致 **{len(mismatch)}** · "
        f"已合并但登记表仍写未完成 **{len(stale_open)}** · "
        f"写已完成但查不到 [Merge] **{len(suspicious_done)}**（另有 {len(unverifiable)} 条历史不可用 git 验证）· "
        f"无 status.md 的目录 **{len(dirs_missing_md)}/{len(dirs)}** · 有目录无登记 **{len(no_registry)}**\n")

    add("## 1 · 同名重复（历史事故复发风险）\n")
    add("| REQ | 各行状态 |")
    add("| --- | --- |")
    for req, sts in dup:
        add(f"| {req} | {' / '.join(sts)} |")
    if not dup:
        add("| —— | 无 |")

    add("\n## 2 · 已合并但登记表仍写未完成（最可能是真漂移）\n")
    add("| REQ | 登记表状态 | 合并提交 |")
    add("| --- | --- | --- |")
    for req, st, m in stale_open:
        add(f"| {req} | {st} | `{m.split(chr(9))[0]}` {m.split(chr(9))[2][:90]} |")
    if not stale_open:
        add("| —— | 无 |")

    add("\n## 3 · status.md 与登记表不一致\n")
    add("| REQ | 登记表 | status.md |")
    add("| --- | --- | --- |")
    for req, reg, md in mismatch:
        add(f"| {req} | {reg} | {md} |")
    if not mismatch:
        add("| —— | 无 |")

    add("\n## 4 · 写已完成但查不到 [Merge] 提交，**却有提交提到它**（需人看）\n")
    add("| REQ | 登记表 | 提交数 | 最近一条 |")
    add("| --- | --- | ---: | --- |")
    for req, st, n, last in suspicious_done:
        add(f"| {req} | {st} | {n} | {last.split(chr(9))[2][:80] if last else ''} |")
    if not suspicious_done:
        add("| —— | 无 |")

    add("\n## 4b · 写已完成但提交历史里**根本没提过它**（多为 2026-10 之前的脱敏快照，git 无法验证）\n")
    add("、".join(r for r, _ in unverifiable) if unverifiable else "无")

    add("\n## 5 · 有目录、登记表无此行\n")
    add("、".join(no_registry) if no_registry else "无")

    add("\n## 6 · 目录存在但没有 status.md（本项目常态；这些 REQ 的状态只有登记表一个来源）\n")
    dirs_missing_md = [d for d in dirs if not (BA / "demands" / d / "status.md").exists()]
    add(f"共 {len(dirs_missing_md)}/{len(dirs)} 个目录没有 status.md")
    add("、".join(dirs_missing_md) if dirs_missing_md else "无")

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(L) + "\n", encoding="utf-8")
    print(f"report: {OUT}")
    print(f"rows={len(rows)} reqs={len(by_req)} dirs={len(dirs)} dup={len(dup)} "
          f"mismatch={len(mismatch)} stale_open={len(stale_open)} suspicious_done={len(suspicious_done)} "
          f"no_status={len(missing_status)} no_reg={len(no_registry)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
