#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
把真实腾讯云响应脱敏成可入库的 fixture（确定性）。

⚠️ 教训：第一版只覆盖了 ins-/img-/sg-/vpc-/subnet-/disk- 六种前缀，
   **漏了 `skey-`（密钥对 ID）**，于是真实密钥对 ID 被留在了夹具里（QA 抓到的 B-1）。

现在改成：
  ① **任何** `<小写字母>-<小写字母数字>` 形式的 ID 一律替换（白名单化为黑名单式的"全都换"）
  ② 加一道**独立校验**：逐字段比对 raw 与 masked，凡是"看起来像标识"的字符串
     **不允许相等**（除非在 KEEP 白名单里）
  ③ 同一输入两次 → 逐字节相同

用法：
  python fixtures/desensitize.py --raw <raw.json> --out <fixture.json>
"""
import argparse
import json
import re
import sys
from collections import OrderedDict

# 通用 ID 形状：小写字母开头的一段 + '-' + 小写字母数字
GENERIC_ID = re.compile(r"^[a-z][a-z0-9]*-[a-z0-9]+$")
UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", re.I)
IPV4 = re.compile(r"^\d{1,3}(\.\d{1,3}){3}$")
IPV6ISH = re.compile(r"^[0-9a-fA-F:]{9,}$")

# 必须保留（语义）的键 —— 这些键的值即使像 ID 也不换
KEEP = {
    "Zone", "InstanceState", "InstanceType", "InstanceChargeType", "CPU", "Memory",
    "RestartState", "RenewFlag", "OsName", "ImageType", "InstanceFamily", "ImageFormat",
    "LatestOperation", "LatestOperationState", "StopChargingMode", "BootMode",
    "RestrictState", "DisableApiTermination", "EnableJumboFrame", "VmEniTrunking",
    "IsolatedSource", "DisasterRecoveryStatus", "CpuTopology", "LicenseType",
    "ApplicationRole", "DefaultLoginPort", "DefaultLoginUser", "TotalCount",
    "Bandwidth", "InternetChargeType", "AllocatePublicIp", "InstanceStateRaw",
}

# 明确要替换成"有语义前缀"的键
PREFIXED = {
    "InstanceId": "ins", "ImageId": "img", "VpcId": "vpc", "SubnetId": "subnet",
    "SecurityGroupId": "sg", "DiskId": "disk", "KeyIds": "skey",
    "RequestId": "req", "LatestOperationRequestId": "opreq",
}

_counter = {}


def bump(k: str) -> int:
    _counter[k] = _counter.get(k, 0) + 1
    return _counter[k]


def walk(node, key=None, parent=None):
    if isinstance(node, dict):
        return OrderedDict((k, walk(v, k, node)) for k, v in node.items())
    if isinstance(node, list):
        return [walk(v, key, parent) for v in node]
    if not isinstance(node, str):
        return node

    s = node
    if key in KEEP:
        return s
    if key == "InstanceName":
        return f"demo-cvm-{bump('name'):02d}"
    if key == "CamRoleName":
        return "demo-cam-role"
    if key in ("OperatorUin", "PlatformProjectId", "ProjectId"):
        return 100000000000

    if key in PREFIXED:
        return f"{PREFIXED[key]}-{bump(PREFIXED[key]):07d}"

    if UUID.match(s):
        return f"00000000-0000-4000-8000-{bump('uuid'):012d}"
    if IPV4.match(s):
        n = bump("ip")
        return f"10.0.{n % 256}.{n // 256 % 256}"
    if IPV6ISH.match(s):
        return f"2402:db8::{bump('ip6'):x}"

    # 通用 ID：**任何** xxx-yyy 形状都换（这是第一版漏掉 skey- 的根因）
    if GENERIC_ID.match(s):
        prefix = s.split("-")[0]
        return f"{prefix}-{bump(prefix):07d}"

    if isinstance(parent, dict) and set(parent.keys()) >= {"Key", "Value"}:
        if key == "Key":
            return s
        if key == "Value":
            return f"demo-tag-{bump('tagv'):02d}"

    return s


# 看起来像"标识"的字符串（用于 raw vs masked 的独立比对）
LOOKS_LIKE_ID = re.compile(r"^([a-z][a-z0-9]*-[a-z0-9]+|[0-9a-fA-F]{8}-[0-9a-fA-F-]{27,}|\d{1,3}(\.\d{1,3}){3})$")


def collect_identity(node, key=None, out=None, path="") -> set:
    """收集 (path, value) 里"像标识"的字符串，跳过 KEEP 键。"""
    if out is None:
        out = set()
    if isinstance(node, dict):
        for k, v in node.items():
            collect_identity(v, k, out, f"{path}.{k}")
        return out
    if isinstance(node, list):
        for i, v in enumerate(node):
            collect_identity(v, key, out, f"{path}[{i}]")
        return out
    if isinstance(node, str) and key not in KEEP and LOOKS_LIKE_ID.match(node):
        out.add((path, node))
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--raw", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    raw = json.load(open(args.raw, encoding="utf-8"), object_pairs_hook=OrderedDict)
    masked = walk(raw)
    json.dump(masked, open(args.out, "w", encoding="utf-8"), ensure_ascii=False, indent=2)

    # ① 结构自检
    src = raw["Response"]["InstanceSet"][0]
    dst = masked["Response"]["InstanceSet"][0]
    assert list(src.keys()) == list(dst.keys()), "字段集合/顺序变了"
    assert len(raw["Response"]["InstanceSet"]) == len(masked["Response"]["InstanceSet"])

    # ② **独立校验**：raw 与 masked 的"像标识的值"不允许相同
    raw_ids = collect_identity(raw)
    same = [(p, v) for (p, v) in raw_ids if walk_value(masked, p) == v]
    print(f"写出: {args.out}")
    print(f"实例数: {len(dst)} · 字段数: {len(dst)} · 每实例字段: {len(dst.keys())}")
    print(f"替换计数: {_counter}")
    print(f"校验① 结构一致: ✅")
    if same:
        print(f"⛔ 校验② 失败：{len(same)} 个标识未被替换")
        for p, v in same[:10]:
            print(f"   {p} = {v}")
        return 1
    print(f"校验② raw vs masked 标识零残留: ✅（检查了 {len(raw_ids)} 个疑似标识值）")
    return 0


def walk_value(node, path: str):
    """按 '.a.b[0]' 形式的 path 取值（仅用于校验）。"""
    cur = node
    for part in [p for p in re.split(r"\.(?![^\[]*\])", path) if p]:
        name = part.split("[")[0]
        if name:
            if not isinstance(cur, dict) or name not in cur:
                return None
            cur = cur[name]
        for idx in re.findall(r"\[(\d+)\]", part):
            i = int(idx)
            if not isinstance(cur, list) or i >= len(cur):
                return None
            cur = cur[i]
    return cur


if __name__ == "__main__":
    sys.exit(main())
