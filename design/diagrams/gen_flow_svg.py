#!/usr/bin/env python3
"""生成 `build-to-consume.svg`（数据驱动：改 STEPS 重出图）。

用法：  python BA/design/diagrams/gen_flow_svg.py

为什么把脚本入库：图是**人读的**，流程一变图就会变成假话。
脚本与图**同一次提交**，改流程时顺手改 STEPS，比"记得回来改图"可靠。
"""
# -*- coding: utf-8 -*-
import html, pathlib

W = 1320
LANES = [("①  构建端 · ontok-builder（FDE 本机，不部署）", 24, "#2f6f9f", "#eaf3fa"),
         ("②  服务端 · ontok（服务器 / k8s）",              456, "#2f7d4f", "#eaf6ee"),
         ("③  消费端 · 数字员工（客户沙箱）",                888, "#8a5a1f", "#fbf3e6")]
LW, BW = 408, 376
BX = [40, 472, 904]
TOP = 84

def esc(s): return html.escape(s, quote=True)

def box(x, y, w, title, lines, color):
    lh, th = 15, 18
    h = th + len(lines) * lh + 12
    out = [f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="9" fill="#fff" stroke="{color}" stroke-width="1.4"/>',
           f'<text x="{x+11}" y="{y+19}" font-family="Microsoft YaHei,Segoe UI,sans-serif" font-size="13" font-weight="700" fill="{color}">{esc(title)}</text>']
    for i, ln in enumerate(lines):
        mono = ln[:1] in "$#GETPOSUT." or ln.startswith("GET") or ln.startswith("POST") or ln.startswith("PUT")
        fam = "Consolas,monospace" if mono else "Microsoft YaHei,Segoe UI,sans-serif"
        fill = "#7a5a00" if ln.lstrip().startswith("#") else "#123"
        out.append(f'<text x="{x+11}" y="{y+th+15+i*lh}" font-family="{fam}" font-size="{11.2 if mono else 11.6}" fill="{fill}">{esc(ln)}</text>')
    return "\n".join(out), h

STEPS = [
 [ # ① 构建端
  ("输入（二选一）", ["文档目录（PDF/MD/docx）  或  源库只读连接串", "# 源库只读；文档不连库"]),
  ("① 抽取 → 草稿（一条命令）", ["$ ontok-builder build run --input <dir> \\",
                         "    --work-dir <work> --project <p> --agent acp \\",
                         "    --agent-command <cli> --agent-skill <skill路径> \\",
                         "    --strict-deterministic",
                         "# 纯本地：不连产品库、不 apply、不定版"]),
  ("①' 或分步（等价）", ["$ ontok-builder doc ingest --input <dir> --out out/",
                   "$ ontok-builder import --proposal out/proposal.json",
                   "$ ontok-builder review decide --proposal <id> …"]),
  ("② 人签 → 定版", ["$ ontok-builder console serve --dir <work>",
                 "# 浏览器：草稿 → 逐条人签 → 定版（vN 冻结）",
                 "# agent 生成的是草稿，人批过才是定版"]),
  ("③ 打包（只收定版）", ["$ ontok-builder bundle pack --dir <work> \\",
                    "    --project <p> --version vN --out a.ontokbundle \\",
                    "    [--am am-tencent.json] [--db-schema schema.json] \\",
                    "    [--requires-dsn] [--db-dialect postgres]",
                    "# store-only zip；⛔ 不带数据"]),
  ("④ 自检（出厂闸）", ["$ ontok-builder bundle verify --file a.ontokbundle",
                  "# exit 0 才发；清单须覆盖每个文件（除 manifest 本身）",
                  "# 非 store 压缩 ⇒ bundle.unsupported.compression"]),
  ("产物", ["a.ontokbundle", "# 本体 snapshot + AM + 源库描述（无数据）"]),
 ],
 [ # ② 服务端
  ("部署（读面 + 写面同一进程）", ["$ ontok query serve --bind 0.0.0.0:8800 \\",
                            "    --tokens /etc/ontok/secrets/query-tokens.json \\",
                            "    --ontology-only  [--project <p> --version vN]",
                            "# 环境：ONTOK_DATABASE_URL + ONTOK_SECRET_KEY(64hex)"]),
  ("① 建项目 + 发 key（admin）", ["POST /api/projects",
                            "  {\"project\":\"<p>\",\"label\":\"…\",\"key_label\":\"…\"}",
                            "⇒ 201 一次性 key（kind=query，一 key 一项目）"]),
  ("② 上传构建包", ["POST /api/projects/<p>/bundles",
              "  Content-Type: application/octet-stream",
              "  body = a.ontokbundle 的原始字节",
              "⇒ 201 bundle_id + sha256_verified=true"]),
  ("③ 配 DSN（仅 requires_dsn）", ["PUT  /api/projects/<p>/database  {\"dsn\":\"…\"}",
                            "POST /api/projects/<p>/database/test",
                            "# 失败也 200（看 ok 字段）；DSN 加密列存储"]),
  ("④ 解析入库", ["POST /api/projects/<p>/bundles/<id>/import",
             "⇒ 200 version=vN（frozen=false）",
             "# 已导入 ⇒ 200 + already_imported:true（幂等）"]),
  ("⑤ 冻结 + 激活（生效）", ["POST /api/projects/<p>/bundles/<id>/activate",
                    "⇒ 200 frozen=true · active_version_id 指向它",
                    "# 不激活 ⇒ 对外 503 project.no_active_version"]),
  ("对外服务面", ["GET /api/health · /api/catalog · /api/describe",
             "POST /api/sparql · GET /api/ontology.ttl · /mcp",
             "Studio: /studio（admin key 才能写）"]),
 ],
 [ # ③ 消费端
  ("连接（部署注入，不写进 skill）", ["ONTOK_BASE_URL=https://<host>",
                            "ONTOK_TOKEN=<query key>（只读）",
                            "# 平台注入的凭据是 SKILL_CREDENTIAL_*"]),
  ("① 探路：先看模式", ["GET /api/health ⇒ mode",
                  "# ontology-only（不接库）/ 普通模式（接库）"]),
  ("② 查本体：拿到取数配方", ["POST /api/sparql",
                      "  {\"query\":\"SELECT ?s WHERE { ?s a <ns#class> } LIMIT 1\"}",
                      "⇒ 200  @graph:[] + @pending[].get"]),
  ("③ 配方内容（v2 契约）", ["get = {class, ref, source{base_url,auth_mode},",
                      "       operation{action,method,path,headers,vars},",
                      "       request{template,paging,defaults},",
                      "       iterator, fields, auth{credential_env,service}}",
                      "# v1 只有 class/filter/pushdown/ref ⇒ 要 AM 文件"]),
  ("④ 凭据桥接（平台沙箱）", ["TENCENT_SECRET_ID=\"$SKILL_CREDENTIAL_SECRETID\" \\",
                      "TENCENT_SECRET_KEY=\"$SKILL_CREDENTIAL_SECRETKEY\" \\",
                      "  python scripts/access_fetch.py …",
                      "# 必须同一命令内；值不落盘不回显"]),
  ("⑤ 取数（配方直接喂脚本）", ["$ python scripts/access_fetch.py --recipe - \\",
                        "    --entity <配方里的 class> \\",
                        "    --id-property <fields 里的属性名> \\",
                        "    --format rows --out rows.json",
                        "# 缺 --id-property ⇒ 每行被 skipped_no_identity 丢掉（0 行）"]),
  ("产物", ["rows.json / JSON-LD",
          "# meta.rows / pages / truncated；truncated ⇒ exit 8"]),
 ],
]

parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="__W__" height="__H__" viewBox="0 0 __W__ __H__">',
 '<defs><marker id="arw" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">'
 '<path d="M 0 0 L 10 5 L 0 10 z" fill="#8a8a8a"/></marker></defs>',
 '<rect width="__W__" height="__H__" fill="#fdfdfd"/>',
 '<text x="24" y="40" font-family="Microsoft YaHei,Segoe UI,sans-serif" font-size="22" font-weight="800" fill="#111">ontok · 从构建到消费的完整流程（v0.3.0）</text>',
 '<text x="24" y="64" font-family="Microsoft YaHei,Segoe UI,sans-serif" font-size="12.5" fill="#555">实线 = 主路径。跨阶段两条箭头：构建端 → 服务端 = <tspan font-weight="700">人工上传构建包</tspan>；服务端 → 消费端 = <tspan font-weight="700">HTTPS + query key</tspan>。$ 开头为命令，# 开头为判据或注意。</text>']

for label, x, color, tint in LANES:
    parts.append(f'<rect x="{x}" y="{TOP}" width="{LW}" height="__LANE__" rx="14" fill="{tint}" stroke="{color}" stroke-width="1"/>')
    parts.append(f'<text x="{x+14}" y="{TOP+26}" font-family="Microsoft YaHei,Segoe UI,sans-serif" font-size="14.5" font-weight="800" fill="{color}">{esc(label)}</text>')

ys = []
bottom = 0
for col, steps in enumerate(STEPS):
    y = TOP + 40
    colmap = {}
    for title, lines in steps:
        svg, h = box(BX[col], y, BW, title, lines, LANES[col][2])
        parts.append(svg); colmap[title] = (y, h); y += h + 16
    ys.append(colmap); bottom = max(bottom, y)

# 跨阶段肘形箭头
def elbow(x1, y1, x2, y2):
    xm = x1 + 26
    return (f'<polyline points="{x1},{y1} {xm},{y1} {xm},{y2} {x2},{y2}" fill="none" '
            f'stroke="#8a8a8a" stroke-width="1.6" marker-end="url(#arw)"/>'), xm

c1, c2, c3 = ys
y1 = c1["产物"][0] + c1["产物"][1] // 2
y2 = c2["② 上传构建包"][0] + 24
poly, xm = elbow(BX[0] + BW + 6, y1, BX[1] - 8, y2)
parts.append(poly)

y3 = c2["对外服务面"][0] + 24
y4 = c3["① 探路：先看模式"][0] + 40
poly2, xm2 = elbow(BX[1] + BW + 6, y3, BX[2] - 8, y4)
parts.append(poly2)

# 底部四道闸
gy = bottom + 8
parts.append(f'<rect x="24" y="{gy}" width="{W-48}" height="188" rx="12" fill="#fff8e6" stroke="#b8860b" stroke-width="1.2"/>')
parts.append(f'<text x="40" y="{gy+26}" font-family="Microsoft YaHei,Segoe UI,sans-serif" font-size="14.5" font-weight="800" fill="#7a5a00">发版 / 出厂的四道闸（缺任何一道都会「看起来成功、实际不可用」）</text>')
GATES = [("① AM 校验", "$ python Deploy/tools/check_am.py", "非 0 ⇒ 卡住不发（AM 是人写的，没有生成器兜底）"),
         ("② 两个前置", "ONTOK_SECRET_KEY(64hex) + admin token", "缺前者拒绝启动；缺后者 Studio 写面全 403"),
         ("③ 同 tag 重建", "imagePullPolicy: Always", "否则老 pod 静默跑旧镜像（tag/pod 名都一样）"),
         ("④ 脚本自检", "$ python scripts/access_fetch.py --selftest", "98 passed / 0 failed（离线，不联网不要凭据）")]
for i, (t, cmd, why) in enumerate(GATES):
    gx = 40 + (i % 2) * 636
    gyy = gy + 50 + (i // 2) * 64
    parts.append(f'<text x="{gx}" y="{gyy}" font-family="Microsoft YaHei,Segoe UI,sans-serif" font-size="12.5" font-weight="700" fill="#7a5a00">{esc(t)}</text>')
    parts.append(f'<text x="{gx+100}" y="{gyy}" font-family="Consolas,monospace" font-size="11.5" fill="#123">{esc(cmd)}</text>')
    parts.append(f'<text x="{gx+100}" y="{gyy+16}" font-family="Microsoft YaHei,Segoe UI,sans-serif" font-size="11" fill="#7a5a00">{esc(why)}</text>')

parts.append(f'<text x="40" y="{gy+170}" font-family="Microsoft YaHei,Segoe UI,sans-serif" font-size="11" fill="#555">取数脚本 9+1 个退出码：am.invalid / am.unsigned / param.not_pushdown / auth.unsupported (2) · credential.missing (3) · auth.rejected (4) · api.throttled (5) · api.invalid (6) · net.timeout / net.error (7) · rows.truncated (8)</text>')
parts.append("</svg>")

H = gy + 188 + 20
svg = "\n".join(parts).replace("__W__", str(W)).replace("__H__", str(H)).replace("__LANE__", str(H - TOP - 20))
out = pathlib.Path(__file__).resolve().parent / "build-to-consume.svg"
out.write_text(svg, encoding="utf-8")
print(f"  已生成 {out.stat().st_size/1024:.1f} KB · 画布 {W}x{H}")
