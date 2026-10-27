#!/usr/bin/env python3
"""skills 可移植性守卫：分发出去的 skill 里⛔ 不许出现本机绝对路径或具体客户目录名。

为什么需要（2026-10-27 实测）：
  某 agent 反复去 `D:/Temp/onto_test_keep/Ops360_Inspector` 找东西 —— 那个目录只存在于
  某台机器上。skill 是**产品交付物**，任何"我这台机器"的路径都会让客户环境里的 agent 空转。

用法：python Deploy/tools/check_skill_paths.py     （exit 0 = 干净；1 = 有违规）
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent / "skills"
PATTERNS = [
    (r"(?<![A-Za-z0-9_])[A-Za-z]:[\\/](?![\\/])", "Windows 盘符绝对路径"),
    (r"(?<![\w:.])/(?:home|Users|workspace|var)/", "POSIX 绝对路径"),
    (r"onto_test_keep",                 "本机测试目录名"),
    (r"Ops360_Inspector",               "具体客户/样例目录名"),
    (r"cloud_res_test",                 "本机测试目录名"),
]
EXTS = {".md", ".py", ".json", ".yaml", ".yml"}


def main() -> int:
    bad = []
    for f in sorted(ROOT.rglob("*")):
        if f.suffix.lower() not in EXTS or not f.is_file():
            continue
        for i, line in enumerate(f.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
            for pat, why in PATTERNS:
                if re.search(pat, line):
                    bad.append((f.relative_to(ROOT.parent), i, why, line.strip()[:100]))
    if not bad:
        print("OK：skills 里没有本机绝对路径 / 具体客户目录名")
        return 0
    print(f"⛔ {len(bad)} 处不可移植：")
    for path, i, why, line in bad:
        print(f"  {path}:{i}  [{why}]  {line}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
