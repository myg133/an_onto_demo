#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
把**已签的全量 access mapping** 投影成 `query serve --access-mapping` 能吃的形状。

⚠️ 为什么需要这个文件（临时手段，不是设计）：

  产品现在有**两份** mapping schema：
    · `ontok access validate`  ← 全量（freshness 是对象 {mode,note}）
    · `query serve --access-mapping` ← 精简（freshness 是字符串）
  两者只差 `freshness` 一个字段，却互不兼容 ⇒ serve 启动即报
  `invalid type: map, expected a string`。

  已开 REQ-150 让产品接受两种形态；落地后**删掉本脚本与其产物**。

设计约束：
  · 只做**投影**，不改任何语义：pushdown/capability/class/id 原样搬
  · `freshness` 对象 → 取其 `mode`（serve 侧只用来展示）
  · 输入必须是**已签**的（reviewed_by 非空），否则拒绝（签了才算数）
"""
import json
import pathlib
import sys

SRC = pathlib.Path(__file__).with_name("am-tencent.json")
DST = pathlib.Path(__file__).with_name("am-tencent.serve.json")


def main() -> int:
    am = json.loads(SRC.read_text(encoding="utf-8"))

    unsigned = [e.get("class") or e.get("id") for e in am["entities"] if not e.get("reviewed_by")]
    if unsigned:
        print(f"⛔ 拒绝：{len(unsigned)} 个 entity 未签 ⇒ 投影出来的东西不该被服务")
        for c in unsigned:
            print(f"   - {c}")
        return 1

    ents = []
    for e in am["entities"]:
        fr = e.get("freshness")
        if isinstance(fr, dict):
            fr = fr.get("mode")
        ents.append({
            "id": e["id"],
            "class": e["class"],
            "capability": {"pushdown": (e.get("capability") or {}).get("pushdown") or []},
            "freshness": fr,
        })

    out = {
        "contract_version": am.get("contract_version", 1),
        "mapping_version": am.get("mapping_version", 1),
        "entities": ents,
    }
    DST.write_text(json.dumps(out, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(f"投影完成: {DST.name}")
    print(f"  entity: {len(ents)} · 源: {SRC.name}（已签 {len(ents)}/{len(am['entities'])}）")
    print(f"  ⚠️ 本文件是**临时投影**，REQ-150 落地后删除")
    return 0


if __name__ == "__main__":
    sys.exit(main())
