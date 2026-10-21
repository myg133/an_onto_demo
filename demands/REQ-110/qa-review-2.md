# QA 复审报告（第 2 轮）：REQ-110 · 内置 TTL 导出 + 页面查看与下载

> **审核对象**：`feature/REQ-110` @ **`2ffac88`**（`638c0dc` 之后的恢复提交），基线 `develop` @ **`abf1c9f`**
> **方式**：**独立克隆** `D:\MyCodes\AI\_qa2_req110`（`git clone --branch feature/REQ-110`，与 feature worktree 零共享、零 git 元数据共享）
> **边界**：未改 feature 分支、未 push、未在 feature worktree 里跑任何写操作
> **范围**：只验四件事（B-1 / A-7 / A-8 / 守卫套件），**不重跑全量审核**；功能面（11 AC）沿用上一轮结论
> **环境**：Windows · rustc/cargo 1.96.0 · `postgresql://postgres:postgres@172.17.0.87:5432/ontok_demo`（本轮四件事均无需 DB）
> **日期**：2026-10-21

---

## 0 · 结论

> ## **仍需修复（FAIL）—— 唯一缺口是 A-7 的那条判据没落地**
>
> | # | 项 | 判定 |
> | --- | --- | --- |
> | 1 | **B-1**：`src/bin/ontok-gate-preflight.rs` 误删 | ✅ **真修好**（提交树里有、与 develop 同一 blob、fresh clone 的 `include_str!` 已解析） |
> | 2 | **A-7**：订正口径 + **新增断言** | ❌ **断言不存在**（`tests/test_req110_ttl.rs` 14 条里没有）⇒ 按裁定 **FAIL** |
> | 3 | **A-8**：23 条分解订正 + G5 | ✅ 指定两处已改；⚠️ A-1/A-3 正文与 `status.md` 残留旧口径（非阻断） |
> | 4 | **三守卫套件** | ✅ **合并后全绿**（3 / 6 / 5）；feature HEAD 单跑 `anchor_guard` 红 1 = 缺 hotfix，不是本需求引入 |
>
> **一句话**：阻断修好了，文档改对了，守卫也绿了；**只差 Dev 补一条测试**（补上并绿 ⇒ 即可 PASS 合并）。

---

## 1 · B-1 复验（逐步证据）

**判据**：fresh checkout 上 `cargo test --test test_retained_names_req084 --no-run` 能否读到文件（上一轮报 `couldn't read ... ontok-gate-preflight.rs`）。

### 1.1 提交树里有没有（有）

```text
$ git rev-parse feature/REQ-110
2ffac88e0f9b5343c9b6099510acc5190fc143c8

$ git show --stat --oneline 2ffac88
2ffac88 [Fix] 恢复误删的 src/bin/ontok-gate-preflight.rs（2758 行）+ 提交报告残留 8 行
 .feature/verification-report.md |    8 +
 src/bin/ontok-gate-preflight.rs | 2758 +++++++++++++++++++++++
 2 files changed, 2766 insertions(+)

$ git ls-tree -r HEAD -- src/bin
100644 blob 2968a892a70838b727e218616d48ecfa15865e9e    src/bin/ontok-gate-preflight.rs
100644 blob 2a019e020880bd5c20c6579231f548eb667aa2b8    src/bin/ontok-introspect.rs
100644 blob ae0e62d5a249ccd60b0a41dc7a2d1c8c4d9ff5d1    src/bin/ontok-propose.rs
100644 blob 401180ac208313a324eb5d2dc683fdba00086dc8    src/bin/ontok-translate-names.rs
```

### 1.2 与 `develop` 是否逐字节一致（是）

```text
$ git ls-tree develop -- src/bin/ontok-gate-preflight.rs
100644 blob 2968a892a70838b727e218616d48ecfa15865e9e    ← 同一个 blob OID

$ git diff develop..HEAD -- src/bin/ontok-gate-preflight.rs
（空，0 行）

$ git hash-object src/bin/ontok-gate-preflight.rs
2968a892a70838b727e218616d48ecfa15865e9e      ← 工作区文件 == 树里的 blob（无 eol/smudge 漂移）
```

⇒ **同一 blob OID + diff 为空 + `hash-object` 相同** = 逐字节一致（2758 行，134,877 字节）。

### 1.3 工作区干净（是，无 `D`）

```text
$ git status --short --branch
## feature/REQ-110
```

没有 `D ` 行，也没有上一轮那种「未跟踪副本」。

### 1.4 fresh clone 的编译检查（文件缺失错误消失；剩下的是 unix 代码）

```text
$ git clone --branch feature/REQ-110 <repo> D:\MyCodes\AI\_qa2_req110
$ git rev-parse HEAD          → 2ffac88e0f9b5343c9b6099510acc5190fc143c8
$ git status --porcelain      → （空）

$ cargo test --test test_retained_names_req084 --no-run
   Compiling ontok v0.1.0 (D:\MyCodes\AI\_qa2_req110)
error[E0433]: cannot find `unix` in `os`
   --> src\bin\ontok-gate-preflight.rs:587:22
    |
587 |         use std::os::unix::fs::MetadataExt;
error[E0599]: no method named `dev` found for struct `Metadata` ...  (×2)
error[E0599]: no method named `ino` found for struct `Metadata` ...  (×2)
error: could not compile `ontok` (bin "ontok-gate-preflight") due to 5 previous errors
warning: build failed, waiting for other jobs to finish...
exit=101
```

**「文件缺失」与「unix 代码」的区分（这是本项的关键）**

| 证据 | 结论 |
| --- | --- |
| 全日志搜 `couldn't read` / `os error 2` / `No such file` → **0 命中** | 上一轮那种错误**不存在** |
| 报错位置是 `:587`–`:588` 的 `std::os::unix`，出错目标是 **`bin "ontok-gate-preflight"`** | 说明该文件**在**，而且**被当成 bin 编译**（文件缺失时连这个 target 都不会有） |
| **同一条命令编出了测试二进制**：`target\debug\deps\test_retained_names_req084-5ad70fbf4149f248.exe`（1,560,064 B，mtime 21:08:17，落在本次运行内） | `include_str!("../src/bin/ontok-gate-preflight.rs")` **已成功解析**；上一轮是测试目标直接编译失败、根本产不出 exe |
| 直接运行该 exe：`4 passed; 2 failed` | 目标可执行，进一步证明编译通过 |

⇒ **B-1 已消除。** 剩下的 Windows 编不过是**环境限制**（该 bin 只在 Linux 编），不是本阻断；
Linux CI 上 `cargo test` 会照常编到它。

> 附注（不计入本轮四项）：直接运行该 exe 时 `test_req084_*` 有 2 红
> （`src/extract/propose.rs` 实测 157 处已登记表名 vs 登记表 130 处）。
> 涉及文件 REQ-110 一个字节没动（`git diff 5737ecf..HEAD` 无 `src/extract/*`、无该测试与 `retained_names.req084.tsv`）
> ⇒ 属**基线遗留**，与本阻断无关，另行处理。

---

## 2 · A-7：订正后的口径与新断言

### 2.1 文档侧 —— ✅ 已写

`acceptance.md` §四 A-7（148–164 行）表述正确：

> 闸扫**数据正文**；头部含有**由项目名派生的前缀**与**计数**，但它们**同时出现在正文**（本体 IRI），排除头部**不减少覆盖**。

并明确要求：「**新增一条断言**：头部 `@prefix` 的 IRI 必须与正文 `<…> a owl:Ontology` 的 IRI **逐字相同**」。

### 2.2 判据侧 —— ❌ **不存在**

`tests/test_req110_ttl.rs`（557 行，**14 条**测试）的清单：

```text
ac001_class_property_and_link_counts_match_the_view           :84
ac002_a_poisoned_note_is_refused_by_the_customer_export       :115
ac002_the_gate_scans_the_serialized_bytes_not_the_struct_fields :143
ac003_two_exports_are_byte_identical_and_order_independent    :157
ac003_the_generation_time_is_not_written_into_the_body        :178
ac008_the_header_declares_all_three_losses_and_the_action_count :198
ac008_the_constant_header_is_outside_the_gated_bytes          :220
modeling_export_is_exempt_but_customer_export_is_not          :233
discipline_h_only_the_pipeline_calls_the_serializer           :283
no_new_dependency_for_turtle                                  :303
ttl_module_is_pure                                            :315
live_anspire_ai_v2_...                                        :336
live_cli_export_refuses_a_poisoned_note_with_a_nonzero_exit   :497
live_cli_export_is_byte_identical_across_two_processes        :536
```

全仓 `@prefix : <` 的**全部**出现位置（`git grep`）：

| 位置 | 是什么 | 是不是 A-7 要求的判据 |
| --- | --- | --- |
| `src/domain/ttl.rs:358` | 生产代码 | — |
| `src/domain/ttl.rs:981`（`base_option_is_normalized`） | 两条**硬编码** `contains`（自定义 base 渲染） | ❌ 不是两个实际 IRI 的相等断言；挡不住「头部新增数据源」；也不在 `test_req110_ttl.rs` |
| `src/server/http.rs:1443`（`ac004_ttl_endpoint_is_gated_and_sets_turtle_headers`） | `body.starts_with("@prefix : <…demo3#> .")` | ❌ 只钉前缀字符串，不读正文本体 IRI |
| `tests/test_req110_ttl.rs:530` | `text.starts_with("@prefix : <https://ontok.local/")`（**截断**匹配） | ❌ 同样不是相等断言 |

再核：`git grep 'a owl:Ontology' -- tests` → **0 命中**（`tests/` 里没有任何测试读过本体 IRI）。
⇒ **「头部前缀 IRI == 正文本体 IRI」这条断言在 `tests/test_req110_ttl.rs` 里不存在，别处也没有等价判据。**

**时间线**（说明这是「要求未落地」，不是「Dev 回退」）：

| 时间 | 提交 | 内容 |
| --- | --- | --- |
| 20:45:31 | `638c0dc` | Dev 交付 |
| 21:04:38 | `2ffac88` | **恢复提交**（只补文件 + 报告 8 行，**没碰 tests**） |
| 21:05:36 | BA `27a5403` | A-6~A-9 裁定，**A-7 才提出这条新断言** |

### 2.3 判定

> ❌ **FAIL**（按本轮规则：文档改了、判据没加 ⇒ FAIL）。
> 「补一条测试」不是可选项：A-7 的订正理由**完全依赖**「头部 IRI 与正文 IRI 逐字相同」这个事实，
> 没有判据钉住它，下次头部一漂移，A-1 的安全论证就无声失效。

---

## 3 · A-8：23 条分解与 G5

| 检查点 | 状态 | 证据 |
| --- | --- | --- |
| `acceptance.md` §四 A-8 | ✅ 已改 | 166–177 行：`投影 11 + filter 11 + TTL 同名合并 1 = 23`；`/api/graph` 844 vs TTL 843；明确「与 graph 同源」这句话**不成立**；要求登记 G5 |
| `design-summary.md` §七 G5 | ✅ 已加 | 103 行：同名属性合并、`required` 取或、843 vs 844、「**TTL 特有的有损**，与投影/剔除无关（QA 实测分解为 11 + 11 + 1）」 |
| `acceptance.md` §三 A-3 正文 | ⚠️ **未同步** | 118–119 行仍写「**12 条**在 `view::project` 投影时丢弃」+「与 `/api/graph`、`/api/node` 是**同一份前处理**」——与 A-8 的裁定直接冲突 |
| `acceptance.md` §三 A-1 表格 | ⚠️ **未同步** | 104 行仍写「头部是**编译期常量**，不含任何项目/对象数据」——A-7 只把订正后的表述放在 §四，§三 原表没标注 |
| `status.md` 交付摘要 | ⚠️ **未同步** | 「差 23 条为投影丢弃 + 属性剔除，**与其它出口一致**」——同上旧口径 |

**判定**：A-8 指定核对的两处 **✅ 成立**；但 A-8 自己写的「裁定：订正 A-3」**没有闭环**（A-3 正文未改），
另有 A-1 表格与 `status.md` 同类残留。**非阻断**，但建议本轮一并改掉，否则同一份文档里两种口径并存。

---

## 4 · 三个守卫套件

### 4.1 结果

| 套件 | `feature/REQ-110` @ `2ffac88`（未含 hotfix） | **合并后**（`2ffac88` + `develop` `abf1c9f`） |
| --- | --- | --- |
| `test_anchor_guard` | **2 passed / 1 failed**（`no_bare_env_manifest_dir_outside_anchor_file`） | **3 passed / 0 failed** ✅ |
| `test_guard_manifest_req081` | **6 passed / 0 failed** ✅ | **6 passed / 0 failed** ✅ |
| `test_read_only_guard` | **5 passed / 0 failed** ✅ | **5 passed / 0 failed** ✅ |

**合并方式**（只在 QA 克隆里做，不动任何真实分支）：

```text
$ git checkout -b qa/merge-req110 2ffac88
$ git merge --no-ff --no-edit develop
Merge made by the 'ort' strategy.
 tests/test_req109_cli_gate.rs     | 4 +++-
 tests/test_req109_e2e_fake_llm.rs | 4 +++-
 2 files changed, 6 insertions(+), 2 deletions(-)      ← 无冲突
HEAD = 9f23f5c（仅存在于克隆）
```

`anchor_guard` 在 feature HEAD 上红，原因确认是**缺 hotfix**：
`git diff 5737ecf..HEAD` 不含 `tests/test_req109_*`；hotfix `abf1c9f`（`dd71e21`）只改那两个文件的裸 `env!`。
⇒ **不是 REQ-110 引入**，与上一轮 N-4 结论一致。**合并后三套件全绿**，满足「纳入不退化」。

### 4.2 绕行披露（按规程：`finally` 挪回 + 双证）

Windows 上 `cargo test --test …` 会连带编 `src/bin/ontok-gate-preflight.rs`（unix-only），必须先挪出。
本轮在**克隆里**执行，`try/finally` 保证挪回：

```text
BEFORE   : sha256=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D  Test-Path=True  len=134877
...（三套件）...
RESTORED : sha256=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D  Test-Path=True  len=134877  equal=True
```

两次运行（feature HEAD / 合并态）的 sha256 都与 Dev 报告 §8 的 `sha after` 一致；
`feature-REQ-110` worktree 全程未被改动（复核：`git status --short` 只输出 `## feature/REQ-110`）。

---

## 5 · 是否需要再修 / 缺什么

### 必须（阻断合并）：补 A-7 的判据（1 条测试，约 15 行）

在 `tests/test_req110_ttl.rs` 增加一条，**机械解析两个 IRI 后断言相等**（不要写成两条硬编码 `contains`）：

```rust
#[test]
fn ac008_the_header_prefix_iri_equals_the_ontology_iri() {
    let doc = serialize_customer(&clean_view(), &opts());
    let text = doc.doc();
    // ① 头部第一行 `@prefix : <IRI#> .`
    let header_iri = text.lines().next().unwrap()
        .strip_prefix("@prefix : <").unwrap()
        .strip_suffix("#> .").unwrap();
    // ② 正文本体节点 `<IRI> a owl:Ontology`
    let onto_iri = doc.body()
        .strip_prefix('<').unwrap()
        .split_once("> a owl:Ontology").unwrap().0;
    assert_eq!(header_iri, onto_iri, "头部前缀 IRI 与正文本体 IRI 必须逐字相同（A-7）");
}
```

建议顺带两条加固（非必需，但能防「判据空转」）：
1. 用**含归一化**的项目名再跑一遍（如 `project = "a-b"` ⇒ 两边都是 `a_b`）；
2. 反向自检：手工拼一个「前缀 ≠ 本体」的字符串，断言上面那个抽 IRI 的 helper 会不相等（证明判据咬得住）。

### 建议（不阻断）：订正三处旧口径

| 位置 | 现在写的 | 应改成 |
| --- | --- | --- |
| `acceptance.md:104`（§三 A-1 表格） | 头部是编译期常量、不含项目数据 | 加一句「（已由 §四 A-7 订正）」 |
| `acceptance.md:118-119`（§三 A-3） | 12 条 + 「同一份前处理」 | `11 + filter 11 + 同名合并 1`；删掉「同一份前处理」 |
| `status.md` 交付摘要 | 「与其它出口一致」 | 同上旧口径 |

### 复验后才可 PASS 的最小清单

```text
1. git ls-tree HEAD src/bin/ontok-gate-preflight.rs              # 非空（已满足）
2. git diff develop..HEAD -- src/bin/ontok-gate-preflight.rs     # 空（已满足）
3. git status --short                                            # 无 D（已满足）
4. cargo test --test test_req110_ttl                             # 15 条（新增 1 条）全绿
5. 合并 develop 后：cargo test --test test_anchor_guard          # 3 passed
   cargo test --test test_guard_manifest_req081                  # 6 passed
   cargo test --test test_read_only_guard                        # 5 passed（均已在合并态实测）
```

---

## 附录 · 本轮复现命令（QA 实测口径）

```powershell
# 独立克隆（不改 feature 分支；克隆内的临时分支只用于「合并后状态」判断）
cd D:\MyCodes\AI
git clone --branch feature/REQ-110 D:\MyCodes\AI\an_onto_demo D:\MyCodes\AI\_qa2_req110
cd D:\MyCodes\AI\_qa2_req110
git branch develop origin/develop

# ① B-1
git ls-tree -r HEAD -- src/bin
git diff develop..HEAD -- src/bin/ontok-gate-preflight.rs
git hash-object src/bin/ontok-gate-preflight.rs
cargo test --test test_retained_names_req084 --no-run   # 只剩 unix 报错；同命令产出 test_retained_names_req084-*.exe

# ② A-7
git grep -n '@prefix : <' -- tests src
git grep -n 'a owl:Ontology' -- tests                    # 0 命中 ⇒ 断言不存在

# ④ 守卫套件（Windows 需 finally 挪回）
$src='src\bin\ontok-gate-preflight.rs'; $bak="$env:TEMP\ontok-preflight-bak.rs"
$h0=(Get-FileHash $src).Hash; Move-Item $src $bak
try { foreach ($t in 'test_anchor_guard','test_guard_manifest_req081','test_read_only_guard') { cargo test --test $t } }
finally { Move-Item $bak $src; "sha=$((Get-FileHash $src).Hash) Test-Path=$(Test-Path $src)" }

# 合并后状态（仅克隆内）
git checkout -b qa/merge-req110 2ffac88; git merge --no-ff --no-edit develop
# 再跑上面三个套件
```

> 克隆 `D:\MyCodes\AI\_qa2_req110` 保留作证据（含 `compile-check.log`），**未 push**、未污染任何真实分支，可随时删除。
