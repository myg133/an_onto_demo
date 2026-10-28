#!/usr/bin/env python3
"""版本一致性守卫：`Cargo.toml` == `Cargo.lock` == 已生成的 npm 包（若有）。

## 为什么需要它

2026-10-27 踩过一次：发版提交把 `Cargo.toml` 从 0.1.0 改成 0.3.0，**忘了改 `Cargo.lock`**
⇒ 任何 `cargo build --locked` 直接失败，而且错误出现在 CI 里、不在发版那一步。

现在发版链又多了一个面（npm 包），版本源更多，所以把这条做成闸：

| 判据 | 说明 |
| --- | --- |
| 1 | `Cargo.toml` 的 `[package] version` 能解析 |
| 2 | `Cargo.lock` 里 `name = "ontok"` 的 `version` 与它**逐字相同** |
| 3 | 若 `npm/out/**/package.json` 存在（本地跑过 `npm/prepare.mjs`），其 version 也必须相同 |
| 4 | 若传了 `--tag vX.Y.Z`，它必须等于 `v<version>`（发布 tag 与包版本不能错位） |

退出码：0 = 一致；2 = 不一致（逐条打印）；1 = 环境问题（读不到文件）。

## 用法

    python Deploy/tools/check_versions.py
    python Deploy/tools/check_versions.py --tag v0.3.1
    python Deploy/tools/check_versions.py --code <code worktree 路径>
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


class Problem(Exception):
    pass


def cargo_toml_version(p: Path) -> str:
    if not p.is_file():
        raise Problem(f"读不到 {p}")
    text = p.read_text(encoding="utf-8")
    m = re.search(r'^version\s*=\s*"([^"]+)"', text, re.M)
    if not m:
        raise Problem(f"{p} 里找不到 [package] version")
    return m.group(1)


def cargo_lock_version(p: Path, crate: str = "ontok") -> str:
    if not p.is_file():
        raise Problem(f"读不到 {p}")
    text = p.read_text(encoding="utf-8")
    # [[package]] 块：找 name = "ontok" 后紧跟的 version
    for block in text.split("[[package]]"):
        if re.search(rf'^name\s*=\s*"{re.escape(crate)}"', block, re.M):
            m = re.search(r'^version\s*=\s*"([^"]+)"', block, re.M)
            if m:
                return m.group(1)
    raise Problem(f"{p} 里找不到 crate `{crate}` 的版本")


def npm_versions(out_root: Path) -> list[tuple[Path, str]]:
    got: list[tuple[Path, str]] = []
    if not out_root.is_dir():
        return got
    for pkg in out_root.rglob("package.json"):
        try:
            data = json.loads(pkg.read_text(encoding="utf-8"))
        except json.JSONDecodeError as e:
            raise Problem(f"{pkg} 不是合法 JSON：{e}") from e
        got.append((pkg, str(data.get("version", ""))))
    return got


def main() -> int:
    here = Path(__file__).resolve().parent
    default_code = here.parent.parent / "code"
    ap = argparse.ArgumentParser(description="版本一致性守卫")
    ap.add_argument("--code", type=Path, default=default_code, help="code worktree 路径（默认 ../code）")
    ap.add_argument("--tag", help="发布 tag（如 v0.3.1）；给了就要求与版本一致")
    args = ap.parse_args()

    try:
        cargo = cargo_toml_version(args.code / "Cargo.toml")
        lock = cargo_lock_version(args.code / "Cargo.lock")
        npm = npm_versions(args.code / "npm" / "out")
    except Problem as e:
        print(f"环境问题：{e}", file=sys.stderr)
        print("提示：需要能看到 code worktree（默认 ../code）；用 --code 指定。", file=sys.stderr)
        return 1

    errs: list[str] = []
    if lock != cargo:
        errs.append(f"Cargo.lock 的 ontok 版本是 {lock}，Cargo.toml 是 {cargo} —— `cargo build --locked` 会失败")
    for pkg, ver in npm:
        if ver != cargo:
            errs.append(f"{pkg.relative_to(args.code)} 版本是 {ver}，Cargo.toml 是 {cargo}")

    if args.tag:
        want = f"v{cargo}"
        if args.tag != want:
            errs.append(f"发布 tag 是 {args.tag}，与版本对不上（应为 {want}）")

    if errs:
        print(f"✗ 版本不一致（{len(errs)} 处）：")
        for e in errs:
            print(f"  - {e}")
        print("\n怎么修：以 Cargo.toml 为唯一源 —— 同步改 Cargo.lock（跑一次 cargo update -p ontok --offline 或 cargo build），"
              "npm 包由 `node npm/prepare.mjs` 重新生成即可。")
        return 2

    extra = f" · npm 包 {len(npm)} 个" if npm else ""
    print(f"✓ 版本一致：Cargo.toml = Cargo.lock = **{cargo}**{extra}" + (f" · tag {args.tag}" if args.tag else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
