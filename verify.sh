#!/usr/bin/env bash
# REQ-002 验收判定器 —— 单一事实源
#
# 为什么要脚本化：AC 曾三次因「命令本身写错」而误判
#   ① 测试名与实际不符 → 零匹配却退出 0 → 假通过
#   ② grep 匹配到文档里写的命令本身 → 假失败
#   ③ `|| echo ✓` 吞掉 grep 的退出码 2（目录不存在）→ 假通过
# 本脚本对这三类都有专门防护。
#
# 用法：在 feature-REQ-002/ 下执行
#   bash BA/verify.sh
# 退出码 0 = 全部通过；非 0 = 有未通过项

set -uo pipefail
# 定位 feature worktree：脚本在 BA/ 下，worktree 与 BA 同级
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKTREE="$(cd "$SCRIPT_DIR/.." && pwd)/feature-REQ-002"
if [ ! -f "$WORKTREE/Cargo.toml" ]; then
  printf "✗ 找不到 worktree: %s\n" "$WORKTREE" >&2
  printf "  提示：先创建 feature-REQ-002 worktree 再验收\n" >&2
  exit 2
fi
cd "$WORKTREE" || exit 2
printf "验收目录: %s\n" "$WORKTREE"
FAILED=0
PASSED=0
UNIMPL=0
RED=$'\033[31m'; GRN=$'\033[32m'; YEL=$'\033[33m'; RST=$'\033[0m'

ok()   { printf "  %s✓%s %s\n" "$GRN" "$RST" "$1"; PASSED=$((PASSED+1)); }
bad()  { printf "  %s✗%s %s\n" "$RED" "$RST" "$1"; FAILED=1; }
# warn = **未实现**：对应任务还没开工，不是「写坏了」
warn() { printf "  %s⚠%s %s\n" "$YEL" "$RST" "$1"; UNIMPL=$((UNIMPL+1)); }
head_() { printf "\n%s%s%s\n" "${1:-}" "${2:-}" "$RST"; }

# ─────────────────────────────────────────────────────────
# 通用守卫：测试必须**真的跑了并通过**
#
# 三个已踩过的坑：
#   ① `cargo test --tests <filter>` 会把 filter 应用到所有 target
#      （含 lib/main 的 "0 passed"），必须用 --test <file> 精确指定
#   ② 零匹配时 cargo 退出码仍是 0 → 必须再查 passed 计数
#   ③ cargo 报 "could not find Cargo.toml" 也是非 0，需区分
# ─────────────────────────────────────────────────────────
require_test() {
  local file="$1" name="$2"
  # cargo 的 --test target 名**不带 .rs 后缀**
  local target="${file%.rs}"
  if [ ! -f "tests/$file" ]; then
    # 三态：文件不存在 = 对应任务未开工（未实现），不是「判定失败」。
    # 判定失效只在「文件存在却零匹配」时发生 —— 那才是真正的假通过风险。
    warn "tests/$file 尚未创建（对应任务未开始）—— 判为「未实现」"
    return 1
  fi
  local out; out="$(cargo test --test "$target" -- "$name" 2>&1)"
  local passed; passed="$(printf '%s' "$out" | grep -oE '^test result: ok\. [0-9]+ passed' | grep -oE '[0-9]+' | head -1)"
  if [ -z "$passed" ] || [ "$passed" = "0" ]; then
    bad "「$name」零匹配 —— 判定失败（不是通过）"
    printf '%s\n' "$out" | grep -E '0 passed|^error' | head -2 | sed 's/^/        /'
    return 1
  fi
  ok "$name 通过（$passed passed）"
  return 0
}

head_ "AC-001 · 测试全绿"
# **第 6 次判定器误判（致命）**：`cargo test | grep -qE 'FAILED'` 在
# `set -o pipefail` 下永远判假 —— grep -q 命中即退出，cargo 收 SIGPIPE
# 非 0 退出 → 管道整体非 0 → `if` 走 else → **测试真的失败也报 ✓**。
#
# 修法：先完整捕获到变量再判，不要在管道里用 `grep -q` 提前退出。
TEST_OUT="$(cargo test 2>&1)"
if printf '%s' "$TEST_OUT" | grep -qE '^test result: FAILED'; then
  bad "存在失败测试："
  printf '%s\n' "$TEST_OUT" | grep -E '^---- .* stdout ----|^test .* FAILED' | head -6 | sed 's/^/        /'
else
  N="$(printf '%s' "$TEST_OUT" | grep -oE '^test result: ok\. [0-9]+ passed' | grep -oE '[0-9]+' | paste -sd+ | bc)"
  ok "cargo test 无失败（共 ${N:-0} 条）"
fi

head_ "AC-002 · 分层纪律：src/domain 零 I/O"
if [ ! -d src/domain ]; then
  bad "src/domain 目录不存在 —— 判定无效（不是通过）"
else
  # **判据在 2026-10-02 独立审核后扩围（B2）**
  #
  # 原判据只黑名单 3 个符号（sqlx|PgStore|db::）。独立审核实验 G4 实测：
  # 往 src/domain/hierarchy.rs 注入一个能编译、不影响任何测试的
  # `std::fs::read_to_string`，本判定器仍输出「✓ src/domain/ 无 I/O 符号」、
  # exit=0 —— **红线被真实 I/O 无声突破，而判定器给全绿**。
  #
  # 覆盖缺口正好落在最常见的 I/O 写法上。故扩为：
  #   标准库  std::{fs,net,process,io,env}
  #   异步   tokio::{fs,io,net,process}
  #   网络   reqwest|ureq|hyper|http
  #   进程   Command
  # 另含 Row/Tuple 的类型名也被 db 段覆盖（sqlx 已含）。
  #
  # **未采用的更彻底方案**：把 domain/ 拆成独立 crate，依赖里不放任何含 I/O 的 crate，
  # 违反直接编译不过。之所以不现在做 —— 它要改 Cargo.toml + 重写全部 24 个文件的
  # import 路径，而本次是退回修复轮，不宜顺带做大规模重构。
  # 补回来时机：判定器判据再被绕过时，或 domain/ 出现第二次架构争议时。
  #
  # 只看非注释、非字符串的代码行；排除 // 与 /// 注释
  # 过滤器先剥掉「[文件:]行号:」前缀再判注释行首。
  # **不能写成 `^\S+:[0-9]+:`** —— 那个模式要求 grep 输出「文件:行号:内容」三段，
  # 而 GNU grep 传**单文件**时会省略文件名前缀，模式整体失配 →
  # 文档注释里出现的关键词会被当成真 I/O 符号（本项目已踩过一次，见 REQ-003）。
  # ── 判据（AC-002 领域层零 I/O）────────────────────────
  # 三个已实测的假失败来源，本判据逐一处理：
  #   ① 行首注释  → 剥前缀后整行以 // /* * 开头
  #   ② 行尾注释  → `code // 注释`，含关键词但不是代码
  #   ③ 字符串字面量 → `"内部实现走 db:: 层"`
  #
  # 做法：把每行的**行尾注释**与**字符串字面量**替换成等长空白再判，
  # 只让**真正的代码**参与匹配。用 sed 的 `s///` 默认行为即可
  # （不写 `p` 标志 = 只替换不打印，替换串为空 = 删除）。
  #
  # `-H` 强制输出文件名：传单文件时 GNU grep 默认省略文件名，
  # 那样前缀变成 `行号:内容`，两段式前缀过滤就失配了（本项目踩过）。
  #
  # 判据要求 `符号::` 形式（`db::` 后紧跟标识符）：光有 `db::` 不足以判定。
  HITS="$(grep -rnHE 'sqlx::|PgStore|db::[a-zA-Z_]|std::(fs|net|process|io|env)|tokio::(fs|io|net|process)|reqwest::|ureq::|hyper::|Command::' src/domain/ \
    | sed -E 's|^[^:]*:[0-9]+:||' \
    | sed -E 's|//.*$||; s|"[^"]*"||g; s|'"'"'[^'"'"']*'"'"'||g' \
    | grep -vE '^[[:space:]]*(//|/\*|\*)' \
    | grep -E 'sqlx::|PgStore|db::[a-zA-Z_]|std::(fs|net|process|io|env)|tokio::(fs|io|net|process)|reqwest::|ureq::|hyper::|Command::' \
    || true)"
  if [ -n "$HITS" ]; then
    bad "src/domain/ 出现 I/O 符号："
    printf '%s\n' "$HITS" | sed 's/^/        /'
  else
    ok "src/domain/ 无 I/O 符号"
  fi
fi

head_ "AC-003 · 快照契约含全部占位（类型 + 子字段）"
# **三态区分**：`snapshot.rs` 尚不存在 = 对应任务未开工（未实现，⚠），
# 与「文件存在但占位被后人清理」（未通过，✗）是两回事。
# 混为一谈会让「还没写」看起来像「写坏了」—— 今天已因判定器误判栽过多次。
if [ ! -f src/domain/snapshot.rs ]; then
  warn "src/domain/snapshot.rs 尚未创建（对应任务未开始）—— 判为「未实现」"
else
  MISSING=""
  for f in action_types functions WriteBinding extracted_by failed_groups disjoint_with \
           key_expr value_map supported_values conflict_field; do
    grep -q "$f" src/domain/snapshot.rs || MISSING="$MISSING $f"
  done
  if [ -n "$MISSING" ]; then
    bad "占位缺失：$MISSING"
  else
    ok "9 项占位齐全（含 WriteBinding 的 4 个子字段）"
  fi
fi

head_ "AC-004 / 005 / 006 / 007 / 009 / 010 / 011 / 014 / 015 · 红线测试"
# file                          test 名
require_test test_no_write_path.rs        test_no_write_path_in_slice1 || true
require_test test_view.rs                 test_view_biz_view_never_exposes_modeling_terms || true
require_test test_apply.rs                test_frozen_version_rejects_writes || true
require_test test_review_state.rs         test_review_link_acceptance_requires_both_ends_accepted || true
require_test test_snapshot.rs             test_snapshot_roundtrip || true
require_test test_snapshot.rs             test_snapshot_records_extracted_by || true
# AC-014/015 的判据是「**apply 时**取谁的值」，所以测试在 test_apply.rs 而不是
# test_snapshot.rs —— ADR-001 定案后这一点变了，判定器跟着改。
# 写成 test_snapshot.rs 时 cargo 会「10 filtered out / 0 passed」，
# 零匹配守卫正确地报了 ✗（这正是它存在的意义）。
require_test test_apply.rs                test_extracted_by_is_not_inherited || true
require_test test_apply.rs                test_failed_groups_not_inherited || true

head_ "AC-009 · 层级测试覆盖率（不接受只剩 1 条）"
LAYER="$(cargo test --test test_hierarchy -- test_hierarchy 2>&1 | grep -oE '^test result: ok\. [0-9]+ passed' | grep -oE '[0-9]+' | head -1)"
if [ -n "$LAYER" ] && [ "$LAYER" -ge 10 ]; then
  ok "层级测试 $LAYER 条（阈值 >= 10，防「删到只剩 1 条」假通过）"
else
  bad "层级测试仅 ${LAYER:-0} 条 —— 覆盖不足"
fi

head_ "人工判据（无法自动判定）"
warn "AC-012 端到端 16 条 CLI 命令 —— 需人工执行（2026-09-30 已实跑通过，见 acceptance.md）"
warn "AC-013 客户视图 3 人判读（不得为开发者）—— 需外部评审人，缺则判「未执行」而非「通过」"

printf "\n"
printf "统计：通过 %s%d%s · 未实现 %s%d%s · 未通过 %s\n" \
  "$GRN" "$PASSED" "$RST" "$YEL" "$UNIMPL" "$RST" \
  "$([ "$FAILED" = "0" ] && printf '无' || printf '有')"
if [ "$FAILED" = "0" ]; then
  printf "%s自动判定项无未通过%s（AC-012/013 需人工）\n" "$GRN" "$RST"
else
  printf "%s有未通过项 —— 需修复后重跑%s\n" "$RED" "$RST"
fi
exit $FAILED
