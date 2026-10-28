#!/usr/bin/env python3
"""凭据环境探针（**本平台专用**）：列出沙箱中已注入的 `SKILL_CREDENTIAL_*` 环境变量。

⚠️ 这个文件属于**集成层**（`Deploy/integrations/digital-employee/`），**不是**产品 skill 的一部分：
   `SKILL_CREDENTIAL_*` 是**本平台的注入约定**，产品 skill 只认 AM 里声明的变量名。

用法（Agent 在沙箱中执行）：
    python3 scripts/env_probe.py

只打印变量名与值长度，⛔ 绝不打印凭据值本身。
退出码恒为 0（探针只做观测，不阻断 skill 其他流程）。
"""

import os


def main() -> int:
    names = sorted(k for k in os.environ if k.startswith("SKILL_CREDENTIAL_"))
    if not names:
        print("NO_SKILL_CREDENTIAL_ENV: 沙箱未注入任何 SKILL_CREDENTIAL_* 环境变量")
        print("排查方向: SKILL.md credentials 声明是否生效 / "
              "员工绑定的 skill 是否为最新版 / 凭据状态是否启用")
        return 0
    print(f"INJECTED_SKILL_CREDENTIAL_COUNT: {len(names)}")
    for name in names:
        value = os.environ.get(name, "")
        print(f"{name}: present, length={len(value)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
