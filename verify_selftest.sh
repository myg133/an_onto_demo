#!/usr/bin/env bash
# 判定器自测（结转自 REQ-002 首轮审核 G-1）
#
# ## 为什么需要这个文件
#
# 作者曾在 REQ-002 的 acceptance.md 里写过一句话：
# 「**判定器本身也是代码，也要有测试**」。
# **这一条从未兑现。** 首轮独立审核临时补了 4 个用例，发现其中 2 个没抓住：
#   - 往 domain/ 注入 `std::fs` I/O  → 判定器 exit=0（假通过）
#   - 删掉 AC-004 红线测试文件        → 判定器 exit=0（假通过）
#
# ## 本脚本干什么
#
# 把「往判定器输入里注入一个已知缺陷，判定器必须报 ✗」这套做法**固化下来**。
# 每个用例 = 一个注入 + 一个期望。**注入没被抓住 = 假通过 = 本脚本失败。**
#
# 这是本项目目前唯一能对抗「判定器自说自话」的机制。
# 改 `verify.sh` / `verify-req003.sh` 之后**必须**跑本脚本。
#
# 用法：bash BA/verify_selftest.sh
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"
WT="$REPO/feature-REQ-003"

RED=$'\033[31m'; GRN=$'\033[32m'; RST=$'\033[0m'
PASS=0; FAIL=0
SANDBOX="$(mktemp -d "${TMPDIR:-/tmp}/vself.XXXXXX")"
trap 'rm -rf "$SANDBOX" 2>/dev/null || true' EXIT

ok()  { printf "  %s✓%s %s\n" "$GRN" "$RST" "$1"; PASS=$((PASS+1)); }
bad() { printf "  %s✗%s %s\n" "$RED" "$RST" "$1"; FAIL=$((FAIL+1)); }

# 造一个最小的 src/domain 沙箱
make_sandbox() {
  # 不用 rm -rf（会被回收站策略拦下，导致「沙箱没清干净」这种假失败）
  find "$SANDBOX/src" -type f -delete 2>/dev/null || true
  mkdir -p "$SANDBOX/src/domain"
  cat > "$SANDBOX/src/domain/mod.rs" <<'EOF'
pub fn pure() -> u8 { 1 }
EOF
}

# 取判定器的 AC-002 判据管道 —— **从脚本原文抽，不手抄**（手抄必漂移）
ac002_predicate() {
  # 与 verify.sh 的 AC-002 判据**保持逐字一致** —— 两份判据漂移本身就是一种缺陷。
  # 三个已实测的假失败来源：行首注释 / 行尾注释 / 字符串字面量。
  # 做法：把行尾注释与字符串字面量删掉再判，只让真正的代码参与匹配。
  sed -E 's|^[^:]*:[0-9]+:||' \
    | sed -E 's|//.*$||; s|"[^"]*"||g' \
    | grep -vE '^[[:space:]]*(//|/\*|\*)' \
    | grep -E 'sqlx::|PgStore|db::[a-zA-Z_]|std::(fs|net|process|io|env)|tokio::(fs|io|net|process)|reqwest::|ureq::|hyper::|Command::' \
    || true
}

# 用 `grep -rHn` 而不是 `find | cat` —— 前者给出 `文件:行号:内容`，
# 过滤器才能用严格的两段式前缀；`cat` 出来的裸代码里 `->` 的冒号会被误剥。
run_predicate() { ( cd "$SANDBOX" && grep -rHn '' src/domain | ac002_predicate ); }

# ── AC-101 判据：只读保证（REQ-003 · S1 从「存在性 grep」改成行为判据）──
#
# ## 为什么要换掉
#
# 原来的写法是「源码里有 `default_transaction_read_only` 这个字符串就算过」
# —— 这是 REQ-002 复审已经栽过的形态：**守卫被删掉、字符串留在注释里，照样 ✓**。
# 独立审核进一步实测：删掉 after_connect 的 SET / 删掉 force_read_only 调用点 /
# 阉割 fail-closed，**148 个测试全绿** —— 只读保证零防回归。
#
# ## 现在的判据
#
# 跑在**剥掉注释、且截止到 `#[cfg(test)]` 之前**的生产代码上，要求三件事同时成立：
#   ① `force_read_only(` 出现 ≥2 次（签名 + **调用点**）
#   ② 有 `after_connect(` 回调，且回调里有 `SET default_transaction_read_only = on`
#   ③ `ensure_read_only(` 出现 ≥2 次（签名 + **调用点**）
# 砍掉 `#[cfg(test)]` 之后是**实测踩过的坑**：不砍的话，单元测试里自己的
# `force_read_only(...)` 调用会顶包，删掉生产接线也照样绿。
#
# 局限（写在明面上，不藏）：只剥行注释（`//`、`///`），
# 不处理跨行 `/* */` —— 与本文件既有的 AC-002 判据同一形态。

# 产出：注释已剥、且只到 #[cfg(test)] 为止的生产代码
ac101_code() {
  awk '/^[[:space:]]*#\[cfg\(test\)\]/{exit} {print}' "$1" | sed -E 's|//.*$||'
}

# 只读判据。0 = 三件事都在；1 = 有缺陷（缺陷逐条打印到 stdout）
#
# **判据 ①③ 从「数出现次数」改成「函数体内查精确调用」**（2026-10-03 修复轮）
#
# 原因：原来是 `grep -c 'force_read_only(' >= 2`（签名 1 次 + 调用 1 次）。
# 独立审核实测两个变异让全绿：
#   R4 池构造删掉真 `force_read_only(url)?`，在别处补一次「像防御纵深」的调用
#   R5 `ensure_read_only(&read_only)?` 改成 `.ok()` —— **真 fail-open**
# 因为生产代码里两者各恰好出现 2 次、卡在阈值上。
# 「计数」判不出「这个调用在不在该在的函数里」，也判不出 `?` 有没有被丢掉。
ac101_readonly_judgement() {
  local f="$1" c pool intro rc=0
  if [ ! -f "$f" ]; then echo "找不到文件 $f"; return 1; fi
  c="$(ac101_code "$f")"
  # 去空白后匹配 → 与排版无关（改格式不会假红）；`?;` 不可省 → `.ok()` 会被抓住
  local sq; sq="$(printf '%s' "$c" | tr -d '[:space:]')"
  local pool_sq intro_sq
  pool_sq="$(printf '%s' "$c"  | sed -E 's/^(.*)fn read_only_pool\(.*/\1/' | tr -d '[:space:]')"
  intro_sq="$(printf '%s' "$c"  | sed -E 's/^(.*)pub async fn introspect\(.*/\1/' | tr -d '[:space:]')"

  printf '%s' "$pool_sq" | grep -q 'force_read_only(url)?;' \
    || { echo "① read_only_pool 函数体内没有 force_read_only(url)?; —— 池构造没真的加只读参数"; rc=1; }
  printf '%s' "$intro_sq" | grep -q 'ensure_read_only(&read_only)?;' \
    || { echo "③ introspect 函数体内没有 ensure_read_only(&read_only)?; —— fail-closed 被绕过（?.ok() 也算）"; rc=1; }
  printf '%s\n' "$c" | grep -q 'after_connect(' \
    || { echo "② 池构造里没有 after_connect 回调"; rc=1; }
  printf '%s\n' "$c" | grep -q 'SET default_transaction_read_only = on' \
    || { echo "② after_connect 里的 SET default_transaction_read_only = on 不在可执行代码里"; rc=1; }
  return $rc
}

# 从真实文件造一个「注入缺陷」的沙箱副本 —— **绝不改真实文件**
# $1=源文件  $2=沙箱目标  $3=sed 表达式
inject_sandbox() {
  mkdir -p "$SANDBOX"
  sed -E "$3" "$1" > "$2"
}

echo "判定器自测 —— 每个用例注入一个已知缺陷，判定器必须报出来"
echo
echo "【AC-002 领域层零 I/O】"

# 用例 1 · 真实 I/O 必须被抓
make_sandbox
cat >> "$SANDBOX/src/domain/mod.rs" <<'EOF'
pub fn leak() -> Option<String> { std::fs::read_to_string("/etc/hostname").ok() }
EOF
if [ -n "$(run_predicate)" ]; then ok "注入 std::fs → 判据命中"; else bad "注入 std::fs → 判据漏了（假通过）"; fi

# 用例 2 · 干净代码不得误报
make_sandbox
if [ -z "$(run_predicate)" ]; then ok "干净代码 → 无误报"; else bad "干净代码被误报：$(run_predicate)"; fi

# 用例 3 · 行首注释里的关键词不得误报
make_sandbox
cat >> "$SANDBOX/src/domain/mod.rs" <<'EOF'
// 这里提到 sqlx 但只是注释
pub fn a() -> u8 { 2 }
EOF
if [ -z "$(run_predicate)" ]; then ok "行首注释含 sqlx → 不误报"; else bad "行首注释被误报：$(run_predicate)"; fi

# 用例 4 · **行尾注释不得误报**（REQ-003 实测补上，REQ-002 复审判定此处为假失败）
make_sandbox
cat >> "$SANDBOX/src/domain/mod.rs" <<'EOF'
pub fn b() -> u8 { 2 } // 详见 sqlx::query
EOF
if [ -z "$(run_predicate)" ]; then ok "行尾注释含 sqlx → 不误报"; else bad "行尾注释被误报：$(run_predicate)"; fi

# 用例 5 · **字符串字面量不得误报**
make_sandbox
cat >> "$SANDBOX/src/domain/mod.rs" <<'EOF'
const NOTE: &str = "内部实现走 db:: 层";
pub fn c() -> &'static str { NOTE }
EOF
if [ -z "$(run_predicate)" ]; then ok "字符串字面量含 db:: → 不误报"; else bad "字符串字面量被误报：$(run_predicate)"; fi

echo
echo "【AC-101 勘查器只读红线】"
INTROSPECTOR="$WT/src/extract/introspect.rs"
if [ -f "$INTROSPECTOR" ]; then
  # 写操作字面量：判据跑在剥掉注释后的代码上
  HITS="$(ac101_code "$INTROSPECTOR" \
    | grep -nE '\b(INSERT|UPDATE|DELETE|TRUNCATE|DROP|ALTER|CREATE)\b' || true)"
  if [ -z "$HITS" ]; then ok "勘查器源码无写操作字面量"; else bad "勘查器出现写操作：$HITS"; fi

  # ── 只读保证：行为判据（原来只是 grep「有没有这个字符串」）──
  # 用例 0：真实文件不得误报
  DEF="$(ac101_readonly_judgement "$INTROSPECTOR")"; RC=$?
  if [ "$RC" -eq 0 ]; then
    ok "只读判据对真实勘查器通过（无缺陷，无误报）"
  else
    bad "只读判据对真实勘查器误报：$DEF"
  fi

  # 用例 1：注入「fail-closed 调用点被删、字符串留在注释里」—— 必须报出来
  INJ="$SANDBOX/introspect-1.rs"
  # sed 替换文本里的 `&` 是「整个匹配」——必须写成 `\&`，否则注入出来的
  # 注释里会把匹配本身再插一遍（第一次就踩了，注入静默变形）
  inject_sandbox "$INTROSPECTOR" "$INJ" \
    's|^([[:space:]]*)ensure_read_only\(&read_only\)\?;|\1// ensure_read_only(\&read_only)?;  ← 注入：守卫已删，字符串留在注释里|'
  # 注入必须真的落在「注释里」——这正是 REQ-002 栽过的绕过形态，
  # 所以这里既要求调用点已消失，也要求那句话还在（否则用例没测到东西）
  if grep -qE '^[[:space:]]*ensure_read_only\(&read_only\)' "$INJ"; then
    bad "注入未生效：沙箱里 fail-closed 调用点仍是可执行代码"
  elif ! grep -qE '^[[:space:]]*// *ensure_read_only\(&read_only\)' "$INJ"; then
    bad "注入未生效：沙箱里找不到被注释掉的 fail-closed 调用"
  elif DEF="$(ac101_readonly_judgement "$INJ")"; [ $? -eq 0 ]; then
    bad "注入「fail-closed 调用点被删 + 字符串留在注释里」→ 判据漏了（假通过）"
  else
    ok "注入 fail-closed 调用点删除（字符串留注释）→ 判据报出：$(printf '%s' "$DEF" | grep -m1 '③')"
  fi

  # 用例 2：注入「after_connect 里的 SET 被删」
  INJ="$SANDBOX/introspect-2.rs"
  S="$(grep -n 'after_connect(|conn' "$INTROSPECTOR" | head -1 | cut -d: -f1)"
  E="$(awk -v s="$S" 'NR>s && /^[[:space:]]*\}\)/{print NR; exit}' "$INTROSPECTOR")"
  if [ -n "$S" ] && [ -n "$E" ]; then
    sed -n "1,$((S-1))p;$((E+1)),\$p" "$INTROSPECTOR" > "$INJ"
    if ac101_readonly_judgement "$INJ" >/dev/null; then
      bad "注入「after_connect 的 SET 被删」→ 判据漏了（假通过）"
    else
      ok "注入 after_connect 的 SET 删除 → 判据报出"
    fi
  else
    bad "定位不到 after_connect 块（行 $S..$E）—— 注入用例失效"
  fi

  # 用例 3：注入「force_read_only 的调用点被删」
  INJ="$SANDBOX/introspect-3.rs"
  inject_sandbox "$INTROSPECTOR" "$INJ" \
    's|^([[:space:]]*)let safe_url = force_read_only\(url\)\?;|\1let safe_url = url.to_string();|'
  if ac101_readonly_judgement "$INJ" >/dev/null; then
    bad "注入「force_read_only 调用点被删」→ 判据漏了（假通过）"
  else
    ok "注入 force_read_only 调用点删除 → 判据报出"
  fi

  # 用例 4：**R4** 池构造删掉真调用、在别处补一次「像防御纵深」的诱饵调用。
  #        这是「数出现次数」判据被绕过的形态 —— 旧判据对它 100% 假通过。
  INJ="$SANDBOX/introspect-4.rs"
  cp "$INTROSPECTOR" "$INJ"
  sed -i -E 's|^([[:space:]]*)let safe_url = force_read_only\(url\)\?;|\1let safe_url = url.to_string();|' "$INJ"
  printf '\nfn _decoy() -> String {\n    let x = force_read_only("postgresql://u:p@h/d");\n    x.unwrap_or_default()\n}\n' >> "$INJ"
  if ac101_readonly_judgement "$INJ" >/dev/null; then
    bad "注入 R4「真调用被删 + 别处补诱饵调用」→ 判据漏了（假通过）"
  else
    ok "注入 R4（诱饵调用绕过）→ 判据报出"
  fi

  # 用例 5：**R5** `ensure_read_only(&read_only)?` → `.ok()`（**真 fail-open**）。
  #        调用还在、出现次数没变，只有 `?` 被丢掉 —— 计数判据对此完全失明。
  INJ="$SANDBOX/introspect-5.rs"
  inject_sandbox "$INTROSPECTOR" "$INJ" \
    's|^([[:space:]]*)ensure_read_only\(&read_only\)\?;|\1let _ = ensure_read_only(\&read_only).ok();|'
  # 注入必须真的生效（否则用例空转，判据「报出」只是因为文件不合法）
  if grep -qE '^[[:space:]]*ensure_read_only\(&read_only\)\?;' "$INJ"; then
    bad "注入未生效：R5 沙箱里仍是 `?` 形式"
  elif ! grep -q 'ensure_read_only(&read_only).ok();' <(tr -d '[:space:]' < "$INJ"); then
    bad "注入未生效：R5 沙箱里找不到 `.ok()` 形式"
  elif ac101_readonly_judgement "$INJ" >/dev/null; then
    bad "注入 R5「`?` → `.ok()`（真 fail-open）」→ 判据漏了（假通过）"
  else
    ok "注入 R5（.ok() 真 fail-open）→ 判据报出"
  fi
else
  bad "找不到勘查器 $INTROSPECTOR"
fi

echo
echo "【AC-105 守门器行为（跑真判据，不 grep 源码字符串）】"
VIEW="$WT/src/domain/view.rs"
if [ -f "$VIEW" ]; then
  # 原来这里是 `grep -q 'MODELING_TERMS_CJK'` / `grep -q 'BUSINESS_ALLOWED_PREFIXES'`
  # —— 只证明「分层常量还在」，证明不了分层**真的按那个语义生效**，
  # 也证明不了「业务概念不误杀」和「未翻的记号必被抓住」。
  # 现在跑客户视图判据的**行为用例**本体：正常业务概念放过、建模术语抓住、
  # 中文术语包含匹配、未翻译的基数记号（AC-005/B1）抓住。
  #
  # **同时做「防空跑」自检**：只看退出码不够 —— 过滤器写错就会「0 passed」
  # 地假通过（B2 同类教训：判据不真正执行就等于没判据）。所以额外要求
  # 输出里确实出现关键用例名与 passed 计数。
  T_OUT="$( cd "$WT" && cargo test --offline --test test_view 2>&1 )"; T_RC=$?
  if [ "$T_RC" -eq 0 ] \
     && printf '%s' "$T_OUT" | grep -q 'test_view_guard_allows_real_business_key_concepts' \
     && printf '%s' "$T_OUT" | grep -q 'test_view_no_cardinality_notation_passes_silently' \
     && printf '%s' "$T_OUT" | grep -qE '[1-9][0-9]* passed'; then
    ok "守门器行为用例实跑通过（含业务概念不误杀 + 基数记号必抓）"
  else
    bad "守门器行为用例没跑起来或未通过（rc=$T_RC）：$(printf '%s' "$T_OUT" | tail -5 | tr '\n' ' ')"
  fi
else
  bad "找不到 $VIEW"
fi

echo
printf "自测结果：通过 %d · 失败 %d\n" "$PASS" "$FAIL"
if [ "$FAIL" = "0" ]; then
  printf "%s══ 判定器自测全过 ══%s\n" "$GRN" "$RST"
else
  printf "%s══ 判定器自测有失败项 —— 判定器不可信 ══%s\n" "$RED" "$RST"
fi
exit $((FAIL > 0))
