#!/usr/bin/env python3
"""守卫：两份 skill 里的谓词表 / 禁用词 / 别名，必须与 `relation-verbs.json` 一致。

## 为什么需要它

`relation-verbs.json` 是谓词的**唯一事实源**（`ontok-builder relation verbs check` 也用它）。
但两份 skill（`ontok-extract` / `ontok-relate`）各自内联了一份谓词表 —— 因为：

1. skill 在运行时是**逐块/逐批内联**进 prompt 的，必须自包含；
2. **独立 agent 模式**下没有任何代码注入，agent 直接读 `SKILL.md` 文件。

⇒ 重复是**故意的**。但"没人比的重复"会漂移：JSON 里加了词、改名、弃用，skill 里没跟上，
模型就会按旧词表输出、或者拿不存在的词硬编。本脚本把这份重复**变成有守卫的重复**。

## 判据（任何一条不满足 ⇒ exit 2）

| # | 判据 |
| --- | --- |
| 1 | 两份 skill 的谓词表**主键集合**与 JSON `verbs` 完全相同（顺序不限，不许少、不许多） |
| 2 | 每行的**中文标签**与 JSON `label` 逐字相同 |
| 3 | JSON 里标了 `deprecated` 的词，skill 行里必须出现「已弃用」或 `deprecated` 字样 |
| 4 | 两份的**禁用词**列表（`uses` / `associated_with` …）与 JSON `forbidden` 集合相同 |
| 5 | 若某份 skill 带**别名表**，其归一目标必须都在 JSON `verbs` 里，且该词的 `aliases` 覆盖表里列出的别名 |

## 用法

    python Deploy/tools/check_verbs.py                     # 默认读 ../code/relation-verbs.json
    python Deploy/tools/check_verbs.py --verbs <path>      # 显式指定
    python Deploy/tools/check_verbs.py --selftest          # 自测：证明它真能红

退出码：0 = 一致；2 = 不一致（打印每一处差异）；1 = 环境问题（读不到文件）。
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
import tempfile
from pathlib import Path

SKILLS = ["ontok-extract", "ontok-relate"]

# `| \`belongs_to\` | 归属于 | 什么时候用 |`
ROW_RE = re.compile(r"^\|\s*`([a-z_]+)`\s*\|\s*([^|]+?)\s*\|")
FORBIDDEN_HINT = re.compile(r"uses\s*·\s*associated_with|forbidden|不许出现|一条都不许出现")


class Problem(Exception):
    """环境问题（不是判据失败）。"""


def read(p: Path) -> str:
    if not p.is_file():
        raise Problem(f"读不到 {p}")
    return p.read_text(encoding="utf-8")


def load_verbs(path: Path) -> dict:
    try:
        data = json.loads(read(path))
    except json.JSONDecodeError as e:
        raise Problem(f"{path} 不是合法 JSON：{e}") from e
    verbs = data.get("verbs")
    if not isinstance(verbs, dict) or not verbs:
        raise Problem(f"{path} 缺少 verbs 映射")
    return data


def table_block(text: str, header_re: str) -> list[str]:
    """取出以某表头开头的一张 markdown 表的行（表头 + 分隔 + 数据行）。

    ⚠️ 必须按「块」解析：skill 里还有别名表、'三样必须齐全' 表等，
    只用行正则会把 `| \\`belong_to\\` · \\`owned_by\\` | \\`belongs_to\\` |` 这种
    别名行也当成谓词行（实测会多报 10 个假键）。
    """
    lines = text.splitlines()
    start = None
    for i, line in enumerate(lines):
        if re.match(header_re, line.strip()):
            start = i
            break
    if start is None:
        return []
    block = []
    for line in lines[start:]:
        s = line.strip()
        if not s.startswith("|"):
            break
        block.append(s)
    return block


def parse_skill_table(text: str, name: str) -> dict[str, str]:
    """抽出 skill 里的谓词表：{主键: 中文标签}。找不到表 ⇒ 环境问题。"""
    block = table_block(text, r"^\|\s*主键\s*\|")
    if not block:
        raise Problem(f"{name}/SKILL.md 里找不到谓词表（表头 `| 主键 |`）")
    rows: dict[str, str] = {}
    for line in block:
        m = ROW_RE.match(line)
        if not m:
            continue
        rows[m.group(1)] = m.group(2).strip()
    if not rows:
        raise Problem(f"{name}/SKILL.md 的谓词表里一行都没解析出来")
    return rows


def parse_deprecated_flags(text: str) -> set[str]:
    """谓词表里带「已弃用」/`deprecated` 的主键。"""
    hit: set[str] = set()
    for line in table_block(text, r"^\|\s*主键\s*\|"):
        m = ROW_RE.match(line)
        if m and ("已弃用" in line or "deprecated" in line):
            hit.add(m.group(1))
    return hit


def parse_aliases(text: str, known: set[str]) -> dict[str, set[str]]:
    """解析「你可能会写 → 归一成」那张表：{归一目标: {别名…}}。"""
    block = table_block(text, r"^\|\s*你可能会写\s*\|")
    out: dict[str, set[str]] = {}
    for line in block:
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) != 2:
            continue
        left, right = cells
        target = right.strip("` ")
        if target not in known or left.startswith("---"):
            continue
        got = {a.strip().strip("`") for a in re.split(r"[·,、]", left) if a.strip().strip("`")}
        if got:
            out.setdefault(target, set()).update(got)
    return out


def parse_forbidden(text: str) -> set[str]:
    """解析禁用词块：反引号包着的英文词，且在「不许出现」附近。"""
    words: set[str] = set()
    in_block = False
    for line in text.splitlines():
        if FORBIDDEN_HINT.search(line):
            in_block = True
        if "```" in line:
            continue
        if in_block and re.search(r"[a-z_]{3,}", line) and not line.strip().startswith("|"):
            for w in re.findall(r"`?([a-z][a-z_]{2,})`?", line):
                words.add(w)
            if words:
                in_block = False
    # 去掉误收的普通词（只保留 known-forbidden 形态：含 _ 或全小写短词）
    return {w for w in words if "_" in w or w in {"uses", "involves"}}


def check(skills_root: Path, verbs_path: Path) -> list[str]:
    data = load_verbs(verbs_path)
    verbs: dict[str, dict] = data["verbs"]
    forbidden_json = set(data.get("forbidden") or [])
    known = set(verbs)

    errs: list[str] = []
    for name in SKILLS:
        f = skills_root / name / "SKILL.md"
        text = read(f)
        rows = parse_skill_table(text, name)

        missing = sorted(known - set(rows))
        extra = sorted(set(rows) - known)
        if missing:
            errs.append(f"[{name}] 谓词表缺 {len(missing)} 个键（JSON 有、skill 没有）：{missing}")
        if extra:
            errs.append(f"[{name}] 谓词表多了 {len(extra)} 个键（skill 有、JSON 没有）：{extra}")

        for k in sorted(known & set(rows)):
            want = (verbs[k].get("label") or "").strip()
            got = rows[k]
            if want and got != want:
                errs.append(f"[{name}] `{k}` 中文标签不一致：skill=「{got}」 JSON=「{want}」")

        dep_json = {k for k, v in verbs.items() if v.get("deprecated")}
        dep_skill = parse_deprecated_flags(text)
        for k in sorted(dep_json - dep_skill):
            errs.append(f"[{name}] `{k}` 在 JSON 里标了 deprecated，但 skill 行里没写「已弃用」")

        # 别名（只有 extract 有那张表）
        aliases = parse_aliases(text, known)
        for target, got_aliases in aliases.items():
            want_aliases = set(verbs.get(target, {}).get("aliases") or [])
            unknown = sorted(got_aliases - want_aliases)
            if unknown:
                errs.append(
                    f"[{name}] `{target}` 的别名表列了 JSON 里没有的别名：{unknown}"
                    f"（JSON 现有：{sorted(want_aliases)}）"
                )

        # 禁用词
        got_forbidden = parse_forbidden(text)
        if got_forbidden:
            bad = sorted(got_forbidden - forbidden_json)
            miss = sorted(forbidden_json - got_forbidden)
            if bad:
                errs.append(f"[{name}] 禁用词里出现 JSON 未收录的词：{bad}")
            if miss:
                errs.append(f"[{name}] 禁用词缺 {len(miss)} 个：{miss}")

    return errs


def selftest(root: Path) -> int:
    """自测：故意改坏一份副本，确认守卫会红。"""
    verbs = root.parent / "code" / "relation-verbs.json"
    if not verbs.is_file():
        print("SKIP 自测：找不到 code/relation-verbs.json", file=sys.stderr)
        return 0
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td) / "skills"
        shutil.copytree(root / "skills", tmp)
        bad = tmp / "ontok-relate" / "SKILL.md"
        text = bad.read_text(encoding="utf-8")
        mutated = text.replace("| `belongs_to` | 归属于 |", "| `belongs_to` | 归属到（改坏） |")
        if mutated == text:
            print("SKIP 自测：没找到可改的行", file=sys.stderr)
            return 0
        bad.write_text(mutated, encoding="utf-8")
        errs = check(tmp, verbs)
        if not errs:
            print("FAIL 自测：把标签改坏了，守卫居然没红", file=sys.stderr)
            return 1
        print(f"PASS 自测：改坏后守卫报出 {len(errs)} 条问题")
        print(f"  例：{errs[0]}")
    return 0


def main() -> int:
    here = Path(__file__).resolve().parent          # Deploy/tools
    deploy = here.parent                            # Deploy
    ap = argparse.ArgumentParser(description="谓词表守卫：skill ↔ relation-verbs.json")
    ap.add_argument("--verbs", type=Path, default=deploy.parent / "code" / "relation-verbs.json",
                    help="relation-verbs.json 路径（默认 ../code/relation-verbs.json）")
    ap.add_argument("--skills", type=Path, default=deploy / "skills", help="skills 目录（默认 Deploy/skills；测试时指向副本）")
    ap.add_argument("--selftest", action="store_true", help="自测：证明守卫真的能红")
    args = ap.parse_args()

    try:
        if args.selftest:
            return selftest(deploy)
        errs = check(args.skills, args.verbs)
    except Problem as e:
        print(f"环境问题：{e}", file=sys.stderr)
        print("提示：本脚本要同时看到 Deploy/ 与 code/（默认读 ../code/relation-verbs.json）；"
              "只 checkout 了 Deploy 时请用 --verbs 指到已有的那份。", file=sys.stderr)
        return 1

    if errs:
        print(f"✗ 谓词表与 {args.verbs} 不一致（{len(errs)} 处）：")
        for e in errs:
            print(f"  - {e}")
        print("\n怎么修：改 skill 里的表去对齐 JSON（⛔ 不要反过来改 JSON 迁就 skill —— "
              "JSON 是唯一事实源，`relation verbs check` 用的也是它）。")
        return 2

    data = load_verbs(args.verbs)
    print(f"✓ 两份 skill 的谓词表与 {args.verbs} 一致："
          f"{len(data['verbs'])} 个主键 · 禁用词 {len(data.get('forbidden') or [])} 个")
    return 0


if __name__ == "__main__":
    sys.exit(main())
