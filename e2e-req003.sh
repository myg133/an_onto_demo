#!/usr/bin/env bash
# REQ-003 抽取 E2E：真实源库 → 提案 → 双轨审核 → 冻结 → 客户视图 → diff
#
# 全程**不写客户库**：源库只读（ontok-introspect 强制），所有写操作落在
# 应用自己的本体库（ONTOK_TEST_DATABASE_URL）。
#
# ## 判据必须参与退出码（B2 · 独立审核阻断项）
#
# 原稿里第 2 步（9 项）与第 6 步（3 项）的量化校验是 **print-only**：
# python 段只打 `✓`/`✗`，**不写 `FAIL`**。实测喂入故意破坏的数据，
# 8 项全 `✗` 而脚本退出码仍是 0 —— 于是 AC-103/104/106/107 的量化判据
# **从来没参与过判定**，「E2E 全绿」是假的。
#
# 现在：每个 python 判据段**有未通过项就 `sys.exit(1)`**，
# shell 侧 `if ! ...; then bad ...` → `FAIL=1` → 脚本非 0 退出。
# **判据不参与退出码，就只是装饰。**
#
# ## 判据模式（不需要源库连接串）
#
#   REQ003_CHECKS_ONLY=1 bash BA/e2e-req003.sh
#
# 复用 `out/e2e-*.json` 既有产物，只跑第 2/6 步的量化判据。
# 用途：改判据后回归「判据是否真的会让脚本变红」，以及审核复现，
# 不必（也不应）为此去连客户源库。
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"
WT="${ONTOK_WT:-$REPO/feature-REQ-003}"
[ -d "$WT" ] || WT="$REPO/code"
echo "  使用工作区: $WT"
cd "$WT" || exit 2

CHECKS_ONLY="${REQ003_CHECKS_ONLY:-0}"

# 本体库（提案/审核/冻结都落这里）
if [ "$CHECKS_ONLY" != "1" ]; then
  # 源库连接串 —— **只读勘查器自己会强制加只读事务**，这里不依赖调用方自觉
  : "${ONTOK_SOURCE_DATABASE_URL:?必须设置源库连接串}"
fi
: "${ONTOK_DATABASE_URL:?必须设置本体库连接串}"
export ONTOK_DATABASE_URL
export ONTOK_TEST_DATABASE_URL="${ONTOK_TEST_DATABASE_URL:-$ONTOK_DATABASE_URL}"

PROJ="e2e_$(date +%s)"
RED=$'\033[31m'; GRN=$'\033[32m'; YEL=$'\033[33m'; RST=$'\033[0m'
FAIL=0
step() { printf "\n%s━━ %s%s\n" "$YEL" "$1" "$RST"; }
ok()   { printf "  %s✓%s %s\n" "$GRN" "$RST" "$1"; }
bad()  { printf "  %s✗%s %s\n" "$RED" "$RST" "$1"; FAIL=1; }
chk()  { if [ "$1" = "0" ]; then ok "$2"; else bad "$2"; fi }
finish() {
  printf "\n"
  if [ "$FAIL" = "0" ]; then
    printf "%s══ REQ-003 E2E 全部通过 ══%s\n" "$GRN" "$RST"
  else
    printf "%s══ REQ-003 E2E 有未通过项 ══%s\n" "$RED" "$RST"
  fi
  exit $FAIL
}

# ── 判据 1：第 2 步（AC-103/AC-107，9 项）──────────────────
# 返回码 = 判据结果。**有未通过项就非 0**（B2 修法：判据必须参与退出码）。
checks_step2() {
  python3 - "$1" "$2" "$3" <<'PY'
import json,sys
TOT=0; FAILS=0
def chk(cond,msg):
    global TOT,FAILS
    TOT+=1
    print(f"  {'✓' if cond else '✗'} {msg}")
    if not cond: FAILS+=1
try:
    pr=json.load(open(sys.argv[1])); st=json.load(open(sys.argv[2])); proj=sys.argv[3]
except Exception as e:
    chk(False, f"产物不可解析：{e}")
    print(f"  —— 第 2 步：{TOT-FAILS}/{TOT} 通过")
    raise SystemExit(1)
items=pr['items']
ents=[i for i in items if i['kind']=='add_entity_type']
links=[i for i in items if i['kind']=='add_link_type']
chk(pr['project']==proj, f"提案 project 字段 == 实际项目（{pr['project']}）")
chk(len(ents)==st['tables_seen'], f"实体 {len(ents)} 个 == 源库 {st['tables_seen']} 张表")
cn=[e for e in ents if e['payload'].get('display_name','').strip()]
chk(len(cn)==len(ents), f"全部实体有中文 display_name（{len(cn)}/{len(ents)}）")
nprop=sum(len(e['payload'].get('properties',[])) for e in ents)
chk(nprop==st['total_properties'], f"属性 {nprop} 个（嵌套在实体 payload 内）")
chk(st['links_dangling']==0, f"关系 {len(links)} 条，悬空 {st['links_dangling']} 条")
eb=pr.get('extracted_by',{})
chk(bool(eb.get('version')), f"extracted_by 带版本：{eb.get('agent')} v{eb.get('version')}")
chk('read_only=on' in str(eb.get('source','')), f"extracted_by 留痕只读勘查：{str(eb.get('source'))[:70]}")
keys={e['payload']['key'] for e in ents}
badl=[l for l in links if l['payload']['from_type'] not in keys or l['payload']['to_type'] not in keys]
chk(not badl, "关系两端均在实体集合内")
names=[e['payload']['key'] for e in ents]
chk(len(names)==len(set(names)), "实体 key 唯一")
print(f"  —— 第 2 步：{TOT-FAILS}/{TOT} 通过")
raise SystemExit(1 if FAILS else 0)
PY
}

# ── 判据 2：第 6 步（AC-104/AC-105，9 项 = 7 项判据 + 2 项防空跑自检）──
# 同样：**有未通过项就非 0**。视图不可解析也算未通过（fail-closed）。
#
# ## 为什么加了 how_many / meaning 两项（X1）
#
# 原稿只判「对象数」「平均属性数」「零属性数」三项，**从不判 `how_many` /
# `meaning` 是不是人话** —— 于是 B1（基数记号零透出）在这个判据层**零敏感**：
# 复审实测把真实产物 51 条 `how_many` 全改回 `N:1`、51 条 `meaning` 全改回
# `belongs_to`，判据模式仍然 exit 0。
# 根因：B1 的真实防线是 `cli.rs` 里守门器的 `exit 2`，而判据模式
# **根本不执行 `ontology show`** —— 那条防线在这个模式下不存在。
# 现在「客户视图里出现未翻译的内部记号」在**判据层面**就失败。
#
# 形态刻意与 Rust 侧守门器（`cardinality_notation_hit` /
# `untranslated_semantic_hit`）**保持一致**：这里判 ASCII 形态
# （`单字符:单字符` / `单字符..单字符`；语义 = 一个汉字都没有却有 ASCII 词元），
# 不判词表。全角 `：` / `∶` 那几种拼法 Rust 侧也抓不到，两边一起留作紧跟项。
checks_step6() {
  python3 - "$1" "$2" <<'PY'
import json,sys,re
TOT=0; FAILS=0
def chk(cond,msg):
    global TOT,FAILS
    TOT+=1
    print(f"  {'✓' if cond else '✗'} {msg}")
    if not cond: FAILS+=1
# 基数记号的**形态**（与 Rust 侧 cardinality_notation_hit 逐条等价）：
# 单字符 + `:`/`..` + 单字符；两侧都不得再接同类字符，且**右端必须紧邻分隔符**。
# 紧邻这一条不是可选的：真实产物里有 12 个 `0: 已停用, 1: 已启用` 这类枚举型
# 属性名（`note` 是客户自己写的自由文本），实测放宽成「冒号后可有空白」
# 会立刻产生 **54 处误杀**。
#
# ⚠ 右端必须**吃掉**那个字符（`[0-9A-Za-z*]`）再断言其后无同类字符。
# 写成 `(?=..)(?!..)` 两个断言同位置互相矛盾 → 整条正则**永远匹配不到任何东西**
# → 判据恒绿、零鉴别力。下面两条 `CANARY_*` 自检就是专门防这个的。
NOTATION=re.compile(r'(?<![0-9A-Za-z*])[0-9A-Za-z*](?:\.\.|:)[0-9A-Za-z*](?![0-9A-Za-z*])')
HAN=re.compile(r'[一-鿿㐀-䶿豈-﫿]')
ASCII_WORD=re.compile(r'[A-Za-z_]')
try:
    v=json.load(open(sys.argv[1]))
    objs=v.get('objects',[]); rels=v.get('relations',[])
except Exception as e:
    chk(False, f"视图不可解析：{e}")
    objs=[]; rels=[]
# 判据自身的自检（防空跑）：这两条形态规则必须真的抓得到东西。
# 规则被改坏成「永远匹配不到」时，下面两项会恒为 ✓ —— 那就是**绿但零鉴别力**。
# 本轮实测踩过：右端写成 `(?=..)(?!..)` 两个同位置断言，整条正则永不可能匹配。
CANARY_NOTATION = NOTATION.search('{"how_many":"N:1"}') is not None
chk(CANARY_NOTATION, "判据自检：基数记号形态规则抓得住 N:1（防空跑）")
CANARY_SEMANTIC = (not HAN.search('belongs_to')) and bool(ASCII_WORD.search('belongs_to'))
chk(CANARY_SEMANTIC, "判据自检：语义形态规则认得出 belongs_to（防空跑）")
props=[len(o.get('properties',[])) for o in objs]
total=sum(props); avg=total/max(len(objs),1)
zero=sum(1 for p in props if p==0)
chk(bool(objs), f"客户视图对象数：{len(objs)}")
chk(avg>=3, f"平均属性数/对象：{avg:.1f}（REQ-002 真实数据基线 2.2，目标 ≥3）")
chk(zero==0, f"零属性对象：{zero} 个（REQ-002 时 14 个）")
# 「关系数」原本是 print-only —— 提成判据，同时让下面两项**非空转**：
# 关系被清空时，下面两个循环一个都不跑，会假绿。
chk(bool(rels), f"客户视图关系数：{len(rels)}")
# AC-105 · 基数记号零透出：形态判定，不是词表
m=NOTATION.search(json.dumps(v,ensure_ascii=False))
chk(m is None, f"客户视图零基数记号透出（命中 {m.group(0) if m else '无'}）")
# AC-105 · 关系语义零透出：形态判定 —— 一个汉字都没有却有 ASCII 词元/下划线
leaked=[r.get('meaning') for r in rels
        if isinstance(r.get('meaning'),str)
        and not HAN.search(r['meaning']) and ASCII_WORD.search(r['meaning'])]
chk(not leaked, f"关系语义全部是中文人话（未翻译 {len(leaked)} 条，样例 {sorted(set(leaked))[:3]}）")
# AC-104 · 属性投影覆盖率 —— **按需求写的口径**判：
# demand.md「拿真实源库的 1068 列跑新投影，统计覆盖率，< 60% 判 ✗」
# ⇒ 分母是**源库列数**（只读勘查的 column_count），不是「平均属性数」那种替代指标。
try:
    sch=json.load(open(sys.argv[2])); ncols=sch['column_count']
except Exception as e:
    chk(False, f"勘查产物不可解析，拿不到源库列数：{e}")
    ncols=0
cov=100.0*total/ncols if ncols else 0.0
chk(ncols>0 and cov>=60, f"属性投影覆盖率 {cov:.1f}%（{total}/{ncols}，AC-104 目标 ≥60%）")
names=[o['name'] for o in objs][:6]
print(f"  对象样例：{'、'.join(names)}")
print(f"  属性样例：{', '.join(p['name'] for p in objs[0].get('properties',[])[:5])}" if objs and objs[0].get('properties') else "  （无属性）")
print(f"  —— 第 6 步：{TOT-FAILS}/{TOT} 通过")
raise SystemExit(1 if FAILS else 0)
PY
}

# ── 判据模式：只跑量化判据，复用既有产物 ───────────────────
if [ "$CHECKS_ONLY" = "1" ]; then
  # 局限（写在明面上）：判据模式没有「真实建出来的项目」，所以第 2 步第 1 项
  # （提案 project == 实际项目）在**这个模式里是自比的**（拿产物自己的 project
  # 当期望值），恒为 ✓。它的鉴别力只能由完整 E2E 跑（`$PROJ` 是真建出来的）
  # 证明 —— 别把判据模式的这一项 ✓ 当成该项已验证。
  step "判据模式 · 第 2 步（AC-103/AC-107，9 项）"
  if checks_step2 "out/e2e-proposal.json" "out/e2e-stats.json" "$(python3 -c "import json;print(json.load(open('out/e2e-proposal.json'))['project'])")"; then
    ok "第 2 步 9 项量化判据全部通过"
  else
    bad "第 2 步量化判据有未通过项（见上方 ✗）"
  fi
  step "判据模式 · 第 6 步（AC-104/AC-105）"
  if checks_step6 "out/e2e-view.json" "out/e2e-schema.json"; then
    ok "第 6 步量化判据全部通过"
  else
    bad "第 6 步量化判据有未通过项（见上方 ✗）"
  fi
  finish
fi

step "0 · 前置检查"
[ -x target/debug/ontok ] || cargo build --offline --bin ontok --bin ontok-introspect --bin ontok-propose 2>&1 | tail -3
for b in ontok ontok-introspect ontok-propose; do
  [ -x "target/debug/$b" ] || { bad "缺二进制 $b"; exit 2; }
done
ok "三个二进制就绪"

step "1 · 只读勘查源库（AC-101/AC-102）"
RO="out/e2e-schema.json"
./target/debug/ontok-introspect --out "$RO" || { bad "勘查失败"; exit 2; }
RO_FLAG="$(python3 -c "import json;print(json.load(open('$RO'))['transaction_read_only'])" 2>/dev/null)"
[ "$RO_FLAG" = "on" ] && ok "transaction_read_only = on（只读留痕在产物里）" || bad "只读未生效：$RO_FLAG"

NTAB="$(python3 -c "import json;print(json.load(open('$RO'))['table_count'])")"
NCOL="$(python3 -c "import json;print(json.load(open('$RO'))['column_count'])")"
[ "$NTAB" -ge 80 ] && ok "勘查到 $NTAB 张表 / $NCOL 列" || bad "表数异常：$NTAB"

# AC-101 静态红线：勘查路径不得出现写操作字面量。
# 过滤器先剥掉「[文件:]行号:」前缀再判注释 —— **不能写成 `^\S+:[0-9]+:`**：
# 那个模式要求 grep 输出「文件:行号:内容」三段，而 GNU grep 传**单文件**时
# 会省略文件名前缀，模式整体失配 → 文档注释里的字面量被当成真写操作。
# （已实测踩过。与 verify_selftest.sh 保持同一写法。）
if grep -nE '\b(INSERT|UPDATE|DELETE|TRUNCATE|DROP|ALTER|CREATE)\b' src/extract/introspect.rs \
   | sed -E 's|^[0-9]+:||' | grep -vE '^[[:space:]]*(//|/\*|\*)' | grep -q .; then
  bad "勘查器源码里出现写操作字面量"
else
  ok "勘查器源码无写操作字面量（AC-101 静态红线）"
fi

step "1.5 · 建项目与基版本 v1"
./target/debug/ontok project new --name "$PROJ" | sed 's/^/    /' || bad "建项目失败"
./target/debug/ontok version new  --project "$PROJ" | sed 's/^/    /' || bad "建版本失败"

step "2 · 抽取本体（AC-103/AC-107）"
PR="out/e2e-proposal.json"
ST="out/e2e-stats.json"
./target/debug/ontok-propose --schema "$RO" --names src/extract/table_names.json \
   --project "$PROJ" --out "$PR" --stats "$ST" || { bad "抽取失败"; exit 2; }
if checks_step2 "$PR" "$ST" "$PROJ"; then
  ok "第 2 步 9 项量化判据全部通过"
else
  bad "第 2 步量化判据有未通过项（见上方 ✗）"
fi

step "3 · 导入提案"
./target/debug/ontok propose import --file "$PR" || bad "导入提案失败"
SHOW="$(./target/debug/ontok review show --project "$PROJ" --proposal p1 2>&1)"
echo "$SHOW" | head -3 | sed 's/^/    /'
ALIAS="$(echo "$SHOW" | grep -oE '\bp[0-9]+\b' | head -1 || true)"
[ -z "$ALIAS" ] && ALIAS="p1"
ok "提案已导入，别名 ${ALIAS:-p1}"

step "4 · 双轨审核全部条目（AC-106）"
NITEMS="$(python3 -c "import json;print(len(json.load(open('$PR'))['items']))")"
echo "    条目数：$NITEMS"
# 一次性接受所有条目 × 两轨 —— 与逐条敲 --item N --track biz/model 等价，
# 走同一条 set_decision 路径与同一套领域校验。
# 逐条敲要 142×2=284 次进程启动、每次重建连接池，实测 15 分钟跑不完。
if ./target/debug/ontok review decide --project "$PROJ" --proposal "${ALIAS:-p1}" --accept-all 2>&1 | sed 's/^/    /'; then
  ok "$((NITEMS*2)) 条 decide 全部成功（批量，${NITEMS} 条目 × 2 轨）"
else
  bad "批量 decide 失败"
fi

STATUS="$(./target/debug/ontok review show --project "$PROJ" --proposal "${ALIAS:-p1}" 2>&1)"
echo "$STATUS" | grep -iE '整体|通过|passed' | head -2 | sed 's/^/    /'
echo "$STATUS" | grep -qE '整体通过：是' && ok "提案整体通过" || bad "提案未整体通过"

step "5 · apply 与冻结（AC-106）"
./target/debug/ontok proposal apply --project "$PROJ" --proposal "${ALIAS:-p1}" || bad "apply 失败"
./target/debug/ontok version freeze --project "$PROJ" --version 2 || bad "冻结失败"
./target/debug/ontok version list --project "$PROJ" | sed 's/^/    /' | head -5

step "6 · 客户视图（AC-104/AC-105）"
./target/debug/ontok ontology show --project "$PROJ" --version 2 > out/e2e-view.json 2>out/e2e-view.err
RC=$?
if [ $RC -eq 0 ]; then
  ok "客户视图产出成功（守门器未拦截 → 无建模术语泄露）"
else
  bad "客户视图失败 rc=$RC：$(head -2 out/e2e-view.err)"
fi
if checks_step6 "out/e2e-view.json" "$RO"; then
  ok "第 6 步量化判据全部通过"
else
  bad "第 6 步量化判据有未通过项（见上方 ✗）"
fi

step "7 · diff（AC-106）"
./target/debug/ontok ontology diff --project "$PROJ" 1 2 2>&1 | head -4 | sed 's/^/    /' && ok "diff 可执行"

step "8 · 冻结后拒一切写入（AC-106 额外验证）"
./target/debug/ontok propose import --file "$PR" >/dev/null 2>&1
P2="$(./target/debug/ontok review show --project "$PROJ" --proposal p1 2>&1 | grep -oE '\bp[0-9]+\b' | head -1 || true)"
[ -z "$P2" ] && P2="p1"
OUT="$(./target/debug/ontok review decide --project "$PROJ" --proposal "${P2:-p1}" --item 1 --track biz --accept 2>&1)"
APPLY2="$(./target/debug/ontok proposal apply --project "$PROJ" --proposal "${P2:-p1}" 2>&1)"; RC2=$?
echo "$APPLY2" | head -2 | sed 's/^/    /'
[ $RC2 -ne 0 ] && ok "基于已冻结 v2 的提案被拒（非 0 退出）" || bad "冻结后仍能 apply"

finish