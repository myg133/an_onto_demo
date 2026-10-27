#!/usr/bin/env python3
"""发版闸：**跟部署走的每一份 AM 都必须过校验**（0 error 才允许发）。

为什么需要（2026-10-27 实测）：
  REQ-166 起 `tc3` 的 `auth.service` 是**必填、fail-closed**（⛔ 不从 base_url 猜），
  而 AM 是**人写的**（`access` 只有 validate/fetch/publish，**没有生成器**）⇒
  没有任何机制会替你发现"这份要发货的 AM 缺了必填项"。

  实测事故：UAT 那份 AM 8 个 source 全缺 `auth.service` ⇒ 直接换镜像**服务起不来**。
  当时靠人手动跑 `--check` 才发现 —— 本脚本就是把那次手动动作固化下来。

用法：
  python Deploy/tools/check_am.py                # 检查有部署的 overlay 下的所有 AM
  python Deploy/tools/check_am.py --file X.json  # 只检查指定文件

退出码：0 = 全部通过；2 = 至少一份有 error（**卡住发版**）。
"""
import argparse
import pathlib
import subprocess
import sys

# Windows 控制台默认 GBK，打 ✓/✗ 会 UnicodeEncodeError ⇒ 强制 utf-8
try:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
except Exception:
    pass

ROOT = pathlib.Path(__file__).resolve().parent.parent          # Deploy/
FETCHER = ROOT / "skills" / "ontok-query" / "scripts" / "access_fetch.py"
OVERLAYS = ROOT / "k8s" / "overlays"


def am_files():
    out = []
    for d in sorted(OVERLAYS.iterdir()):
        if not d.is_dir():
            continue
        # 跟部署走的 AM：overlay 目录下的 am-*.json
        out.extend(sorted(d.glob("am-*.json")))
    return out


def check(path):
    """跑一次 --check，返回 (ok, 摘要行)。"""
    proc = subprocess.run(
        [sys.executable, str(FETCHER), "--check", "--mapping", str(path)],
        capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    body = (proc.stdout or "") + (proc.stderr or "")
    summary = next((l.strip() for l in body.splitlines() if l.strip().startswith("结果：")), "")
    return proc.returncode == 0, summary


def main():
    ap = argparse.ArgumentParser(description="发版闸：校验随部署发货的 AM")
    ap.add_argument("--file", action="append", default=[], help="只检查指定 AM（可重复）")
    args = ap.parse_args()

    targets = [pathlib.Path(f) for f in args.file] if args.file else am_files()
    if not targets:
        print("⚠ 没找到任何 AM（overlays/*/am-*.json）；可能是路径变了，别当成通过")
        return 2

    bad = []
    for p in targets:
        if not p.is_file():
            print(f"  ✗ {p} 不存在")
            bad.append((p, "文件不存在"))
            continue
        ok, summary = check(p)
        try:
            shown = p.resolve().relative_to(ROOT)
        except ValueError:
            shown = p
        print(f"  {'✓' if ok else '✗'} {shown}  {summary}")
        if not ok:
            bad.append((p, summary))

    if bad:
        print(f"\n⛔ {len(bad)} / {len(targets)} 份 AM 没通过 —— **不发版**。")
        print("   修法：照上面的错误逐条改 AM（`am.invalid` 会点名缺哪个键）；改完重跑本脚本。")
        return 2
    print(f"\nOK：{len(targets)} 份 AM 全部通过，可以发版。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
