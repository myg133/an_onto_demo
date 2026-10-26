#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
抓一份本体快照，产出 ui-prototype/snapshot.js（供 file:// 离线预览）。

前置：
  · 能访问目标部署。推荐先端口转发：
        kubectl -n <ns> port-forward svc/ontok-query 18800:8800
  · token 文件路径见下方 TOK_PATH

⚠️ 快照只含**本体元数据 + 少量真实响应**（类/属性/关系/describe/@pending），
   **不含业务数据、不含凭据**。可随时重生成。
"""

import json, urllib.request, urllib.error, pathlib

BASE = "http://127.0.0.1:18800"
TOK = open(r"D:\MyCodes\AI\an_onto_demo\.local\ops360-deploy\partner-token.txt", encoding="utf-8").read().strip()

def post(path, body):
    req = urllib.request.Request(BASE + path, data=json.dumps(body).encode(),
        headers={"Authorization": "Bearer " + TOK, "Content-Type": "application/json",
                 "Accept": "application/ld+json"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())

samples = {}
st, body = post("/api/sparql", {"query": "SELECT ?c WHERE { ?c a :cvm_instance } LIMIT 3"})
samples["cvm_jsonld"] = {"status": st, "body": body}
st2, body2 = post("/api/sparql?format=rows", {"query": "SELECT ?c WHERE { ?c a :cvm_instance } LIMIT 3"})
samples["cvm_rows_error"] = {"status": st2, "body": body2}
st3, body3 = post("/api/sparql", {"query": "SELECT ?c WHERE { ?c a :alarm_notice } LIMIT 3"})
samples["db_class_503"] = {"status": st3, "body": body3}
print(f"  cvm jsonld : {st}  keys={list(body.keys())}")
print(f"  cvm rows   : {st2} code={body2.get('code')}")
print(f"  db class   : {st3} code={body3.get('code')}")

p = pathlib.Path(r"D:\MyCodes\AI\an_onto_demo\UI\studio-preview\snapshot.js")
txt = p.read_text(encoding="utf-8")
txt = txt.rstrip().rstrip(";") + ';\n'
# 追加样本
snap = json.loads(txt[len("window.ONTOK_SNAPSHOT = "):].rstrip().rstrip(";"))
snap["samples"] = samples
p.write_text("window.ONTOK_SNAPSHOT = " + json.dumps(snap, ensure_ascii=False) + ";\n", encoding="utf-8")
print(f"  快照已更新: {p.stat().st_size/1024:.0f} KB")
