# REQ-043 验收报告（QA 独立验证）

- 验收人：verifier（QA）
- 工作区：`/home/myg133/Projects/Anchnet/Ontology/verify-REQ-043`
- 被验 HEAD：`a1089df106a6e0b09fa9db8c105af9f44aca553e`（tree `914cd2d48a39bb51d3e8aa843497ef47eb68de67`）
- 基线：`14e0f1ecd8ba7ff4963847a25add0f01e7a8b069`（tree `bff4a001e7142d7a8c8645ab4b60ee2c2a4e513d`）
- 判据二进制：`test_connect_retry-d11c21d529514294`（每次变异跑的都是它）

## 结论

**PASS 带非阻断**（AC 字面达标；另有 4 条独立构造的**存活假绿**，其中 1 条是 C3b 同类回归，建议 REQ-044 前必修）

---

## 1. 六条存活变异真的全红了吗？是。

口径：命令一律为 `cargo test --test test_connect_retry`（worktree 根 = 仓库 `code/`）。
`ran=12` 表示二进制真的跑起来（`running 12 tests`）；`cf=False` 表示无编译失败。**编译失败一律不计为变异执行。**

| 变异 | 构造 | fixed(a1089df) | 杀死它的判据（位置） |
|---|---|---|---|
| BA | 行尾注释裸 `)` + 其后放 `migrate` | RED 11P/1F | `test_connect_retry.rs:489`（禁用词断言，`assert!` 行） |
| BB | 块注释裸 `)` + 其后放 `migrate` | RED 11P/1F | `:489` |
| BC | 行尾注释裸 `(` + migrate | RED 11P/1F | **`:489`** |
| C3 | `0..=2usize` → `0..=0usize`（零重试） | RED 11P/1F | `test_lib_unit_test_connect_also_retries`，`:791`（`attempts >= 2`） |
| C3b | 退避整段删掉 | RED 11P/1F | `:803`（`loop_body.contains("sleep")`） |
| R3b | helper 改名 `qa_hook` 且不在 pool.rs | RED 11P/1F | `:511`（白名单「解析不到干净体」） |
| R3b_a | helper 改名 `qa_hook` 且**在** pool.rs 体内藏 migrate | RED 11P/1F | `:521`（体里出现禁用词） |

### BC 是不是「真红」？是。

`BC` 在 fixed 树的 panic 落点是 `tests/test_connect_retry.rs:489`（`FORBIDDEN` 循环里的 `assert!`），
**不是**旧树的 `:293`（`balanced_call` 的「括号没有配平」panic）。即：注释不再让配平失控，
配平拿到的是**完整实参**，禁用词被正常看到。**真红，不是 panic 逃逸。**

对照组（良性裸 `(`，无禁用词，变异 `BCb`）：fixed 树 **GREEN 12P/0F**。
同一构造在基线树是 RED 10P/2F @ `:293`（panic）⇒ **假红已被消除，且未引入新的假红。**

### 证明它们是「判据有洞」而非「等价变异」：同构造跑基线树

口径：先 `git checkout 14e0f1e -- tests/test_connect_retry.rs` 换回旧判据，其余源码不变。

| 变异 | 基线 14e0f1e | fixed a1089df | 性质 |
|---|---|---|---|
| BA | **GREEN 12P/0F** | RED @489 | 假绿 |
| BB | **GREEN 12P/0F** | RED @489 | 假绿 |
| BC | RED 10P/2F @293(panic) | RED @489 | 假红→真红 |
| C3 | **GREEN 12P/0F** | RED @791 | 假绿 |
| C3b | **GREEN 12P/0F** | RED @803 | 假绿 |
| R3b | **GREEN 12P/0F** | RED @511 | 假绿 |

与 Dev 自述一致：BC 在基线是 **10P/2F**（我实测同为 10P/2F），其余 12P/0F。
另建 **MID 树**（基线判据 + 只把 `join(" ")` 换成 `join("\n")`）验证 Dev 抓到的换行折叠陷阱：
BA 在 MID 树 **GREEN 12P/0F**、BCb 在 MID 树 **RED 10P/2F @293**
⇒ `join(" ")` 确实是行注释语义塌陷的成因，Dev 的这一发现**属实**。

### Dev 补充变体（全部独立复现）

| 变异 | fixed | 基线 |
|---|---|---|
| BD 嵌套块注释 `/* /* ) */ */` + migrate | RED @489 | GREEN 12P/0F |
| BE2 `'static` 生命周期 + migrate | RED @489 | — |
| BF 字符字面量 `')'` + migrate | RED @489 | GREEN 12P/0F |
| 行注释内含双引号与裸 `)` + migrate | RED @489 | GREEN 12P/0F |
| BG 控制轮：`|| async { Ok(..) }`，`Ok` 在白名单 | GREEN 12P/0F | — |
| BG 控制轮：同上但把 `Ok` 移出白名单 | RED @511 | — |

BG 的因果**证实**：`Ok` 白名单项是**承重**的（删掉即假红），Dev 的自述成立。

---

## 2. 注释处理有没有新洞？主体很扎实，但有 1 条存活假绿。

### 良性构造（必须保持绿）—— 全部通过

一条大杂烩 `G_torture`，一次覆盖：
行尾注释含裸 `)` `(` + `don't` + `/`；块注释 `/* ) ( */`；**嵌套**块注释 `/* /* ) */ */`；
文档块注释 `/** ( */`；字符串含 `//` 与 `/*`（含 `"http://…?a=(b)&c=/*d*/"`）；
raw string `r"( )"` 与 `r#"( )"#`；字符字面量 `'/'` `'('` `')'` `'"'` `'\n'` `'\''`。
结果 **GREEN 12P/0F**。

`G_crlf`：把 `tests/common/mod.rs` 整体转成 CRLF → **GREEN 12P/0F**
⇒ `normalize` 用 `join("\n")` **不会吞掉整个文件**，`\r\n` 也不出问题
（`str::lines()` 已吃掉 `\r`，`trim` 再兜一层）。

### 设计核实：注释紧贴禁用词

`G_forbidden_comment`：被重试闭包内 `let _mc = max_connections; // 这里绝对不能放 migrate`
⇒ **RED @489**。即注释里的禁用词**照旧算命中**，保守方向成立（fail-closed）。

**但 Dev 给这个设计的理由是错的**，值得记一笔（`:234-236`）：
Dev 说「剥掉注释会让『禁用词写在注释里』变绿，造出新的假绿」。不成立 ——
注释里的 `migrate` **根本不执行**，剥掉它只会**减少假红**，不会新增假绿。
保守的选择本身可以接受，**论证不成立**。真正该担心的是反向问题（见 F-3：`normalize` 只剥整行注释、
不剥行尾注释 ⇒ 注释文本会进入配平窗口并参与 `called_idents` 扫描）。

### 【存活假绿 F-1】raw string 内含 `"` 会让扫描器失步

构造 `G_rawstr_hole`：
```
retry_connect(|| async {
    let _mc = max_connections;
    let _r = r#"a"b )"#;              // 合法 raw string，内容 = a"b )
    let p = pool::connect_for_tests(&url, max_connections).await.unwrap();
    ontok::db::pg::migrate(&p).await.unwrap();
    Ok(p)
})
```
⇒ **GREEN 12P/0F**（假绿）。

机理：`r#"` 开了字符串态，内层 `"` 提前闭合 ⇒ 扫描器回到代码态 ⇒ raw string 里真正的 `)`
被当成代码 ⇒ `balanced_call` **提前归零** ⇒ `migrate` 落在窗口外。
反向也成立（raw string 里的 `(` ⇒ 永不归零 ⇒ panic 假红）。

Dev **主动声明过未处理 raw string**，属实且诚实。但它仍是**判据有洞**，不是等价变异
（`migrate` 确实进了重试范围）。属 REQ-041 起的遗留洞，非本次引入。

---

## 3. 次数/退避判据钉住性质了吗？**次数钉住了，退避没有。**

| 绕过构造 | 结果 | 落点 |
|---|---|---|
| `while` + 计数器替代 `for`（`W1_while`） | RED 11P/1F | `:777`「找不到 `for … in 0..=N` 形状」⇒ **cannot certify** |
| `sleep` 挪到循环外（`W2`） | RED 11P/1F | `:803` |
| `tokio::time::sleep` → `tokio::time::interval`（`W3`） | RED 11P/1F | `:803` |
| 上界改 `usize::MAX`（`W4`） | RED 11P/1F | `:785`「上界推不出尝试次数」⇒ **cannot certify** |

**「解析不出来 ⇒ 报 cannot certify 变红」这条兜底真实存在且生效**（`W1`/`W4` 两处独立触发）。

### 【存活假绿 F-2，本轮最严重】退避判据仍是 `contains("sleep")`，注释即可满足

`test_connect_retry.rs:804` = `loop_body.contains("sleep")`。这是**字面量匹配** ——
正是 REQ-043 要从次数性质上铲掉的那种写法，只是留在了退避这一半。

**W5** —— 退避**真删了**，但行尾注释里写了 `sleep`：
```
for attempt in 0..=2usize {
    if attempt > 0 {
        let _ = 0; // 退避原本在这里：tokio::time::sleep(..).await
    }
}
```
⇒ **GREEN 12P/0F**。行为上退避已不存在（无背压紧循环重试），判据看不见。

**W5b** —— 一条注释同时击穿**两个**性质：
```
for _warm in 0..=2usize { let _ = _warm; // 原本这里是 tokio::time::sleep(..) 退避 }
for attempt in 0..=0usize { let _ = attempt; ... }   // 真身：1 次尝试、零退避
```
`retry_loop` 取**第一个** `for` ⇒ 次数推出 3（≥2 且 ≤5 ✓）、循环体里有 `sleep` ✓
⇒ **GREEN 12P/0F**，而真身已经退回零重试 + 零退避。

**论证：这是判据有洞，不是等价变异** —— 被重试行为确实退化了，判据却全绿。
与 C3b 属同一类（C3b = 「退避整段删掉仍绿」，W5/W5b = 「退避整段删掉 + 留个提到 sleep 的注释仍绿」）。
**REQ-043 的 F-3 只把「次数」从字面量升级成性质，「退避」这一半没动。**
修法很轻：退避判据改用文件里现成的 `called_idents(&loop_body)` 找真正被调用的 `sleep`，
而不是 `contains`。

---

## 4. 白名单制有没有把洞换成新洞？**换了一个同类的。**

### 先核实「名字匹配已删」：确已删净

`grep -n '\.contains('` 全量列出（`:502` `CALLED_IDENTS_ALLOWED.contains` 是白名单本身）；
判据里**已无** `ident.contains("connect")` 一类按名字开洞的分支。✔

### 白名单不会放过真实 migrate（无误报面）

BA/BB/BC → `:489`；R3b_a → `:521`；`G_forbidden_comment` → `:489`。
真实 `migrate` 调用三条路径全被拦。

### 【存活假绿 F-3】白名单按**名字**放行，Dev 写下的标准却没被代码强制

`CALLED_IDENTS_ALLOWED`（`:550-555`）含 `Ok/Err/Some/None`。Dev 的注释写着
「白名单的标准是**它不是本仓定义的函数**」—— 但**代码从不校验这一点**。

`G_whitelist_Some`：
- 在 `src/db/pool.rs` 定义 `pub async fn Some(r: Result<PgPool, DbError>) -> Result<PgPool, DbError>`，体内 `migrate`；
- `tests/common/mod.rs` 里 `retry_connect(|| async { pool::Some(pool::connect_for_tests(&url, max_connections).await).await })`

⇒ **GREEN 12P/0F**。`Some` 命中白名单 ⇒ `fn_body` 压根不被调用 ⇒ 真的 `migrate` 被完全跳过。

**这是 Dev 亲手拆掉的那个洞的原样复发**：上一版是「按攻击者可控的名字**开洞**」，
这一版是「按攻击者可控的名字**豁免**」，机制同构。
修法约 3 行：白名单命中时再断言 `fn_body(pool_src, ident).is_none()`（名字在本仓有定义 ⇒ 不许豁免）。

### 【存活假绿 F-4】`fn_body` 可以被**注释**满足

`normalize`（`:237`）只剥**整行** `//` 注释，**行尾注释全部保留**；
`fn_body`（`:584`）取**第一个** `fn <name>(` 匹配就 `return`。

`G_fnbody_comment`：在 `src/db/pool.rs` 靠前位置放一个行尾注释
`use std::time::Duration; // fn qa_hook() { 0 }`，文件末尾再放**真的**藏 `migrate` 的 `qa_hook`，
调用点改成 `pool::qa_hook(&url, max_connections)`
⇒ **GREEN 12P/0F**（`fn_body` 解析到了那段注释的体，真身从未被看）。

性质：**REQ-041 起就存在的洞，但 REQ-043 把 `fn_body` 提升为「每一个被调标识符」的唯一执法路径**，
半径因此显著变大 —— 现在它对**所有** idents 都承重。风险现实：有人在行尾注释里贴一段
`fn foo(...)` 代码片段，就会静默让真身免检。

---

## 5. `test_db.rs` 确实只改了注释吗？是（独立核实）

口径：`git show <rev>:tests/test_db.rs` 分别取出基线版与 HEAD 版，
用 `sed 's://.*::'` 去掉行注释后再去空行，逐字节比对。

- 两侧去注释后均 **18307 字节**
- 两侧 md5 均 `1f2070bca3c37c999c9de8aab9cc6d5c`，`diff` 输出为空

⇒ **0 非注释行改动成立**（Dev 的 F-6 订正属实）。

补充：`git diff --name-only 14e0f1e a1089df -- src/ | wc -l` = **0**
⇒ `src/` 零改动，重试行为未动，改动面仅 `tests/test_connect_retry.rs`(+310/-…) 与 `tests/test_db.rs`(+22 注释)。

---

## 6. 我上一轮对 R3b 的成因解释错了吗？**错了，Dev 是对的。**

我上轮说「`connect` 只出现在行注释里，所以反向确认 `arg.contains("connect")` 白过了」。**不成立。**

我的原构造里**一条注释都没有**，helper 改名后 `arg` 就是 `(|| pool::qa_hook(&url, max_connections))`，
而它在基线树**全绿** —— 说明反向确认是被满足的，满足它的只能是 `max_connections` 里的子串 `connect`。

**决定性实验 `R3b_c`**：helper 改名 `qa_hook`，并把 `max_connections` 一并换成字面量 `1`
（此时 arg 里**既无 `connect` 子串、也无任何注释**）：
- 基线树 → RED 11P/1F @ `:355`（`assert!(arg.contains("connect"))`，即反向确认本身）
- fixed 树 → RED 11P/1F @ `:483`（同一断言）

⇒ 反向确认的唯一承重来源就是 `max_connections` 的子串匹配。
**Dev 的更正确认，我的成因解释作废。** 这条已用于修正我后续需求的判读口径。

---

## 全量 / 红线 / 产物

| 项 | 值 |
|---|---|
| 全量命令 | `cargo test`（worktree 根，`ONTOK_TEST_DATABASE_URL` 由 `.env` 导出并把库名 `notol` 替换为 `ontol`） |
| 全量结果 | **536 passed / 0 failed / 1 ignored**，36 个 `test result:` 行 |
| 全量墙钟 | `real 5m55.145s`（`user 1m1.095s`） |
| 环境类失败 | **0 条**。`test_project` 本轮 10P/0F（Dev 报的 `ConnectionReset` 4 条本轮未复现）⇒ 属负载相关，非代码缺陷 |
| 判据二进制基线 | `test_connect_retry` 12P/0F / 1.51s |
| 变异执行次数 | **47 次**，每次都跑到二进制（`cf=False` 且 `ran=12`） |
| 每次运行后 worktree 干净 | **47/47 True** |
| 被验 HEAD | `a1089df106a6e0b09fa9db8c105af9f44aca553e` |
| 被验 tree | `914cd2d48a39bb51d3e8aa843497ef47eb68de67` |
| 基线 tree | `bff4a001e7142d7a8c8645ab4b60ee2c2a4e513d` |
| 结束态 | worktree `git status --porcelain` 空，HEAD = `a1089df`，`target` 符号链接完好 |
| push | 未执行（仓库无 remote，`git remote -v` 行数 = 0） |

复现率随共享集群负载浮动，**不声称稳定值**。

---

## 发现但未修的问题（按严重度）

| # | 严重度 | 问题 | 位置 | 建议 |
|---|---|---|---|---|
| F-2 | **高** | 退避判据仍是 `contains("sleep")`，注释即可满足；`W5b` 一条注释同时击穿次数+退避两个性质 | `tests/test_connect_retry.rs:804` | 改用 `called_idents(&loop_body)` 找真正被调用的 `sleep`；并让 `retry_loop` 不只取第一个 `for` |
| F-3 | 中 | 白名单按名字豁免，Dev 写下的「不是本仓定义的函数」标准未被代码强制；`pool::Some` 可藏 migrate | `:550-555` / `:502` | 白名单命中时加断言 `fn_body(pool_src, ident).is_none()` |
| F-4 | 中 | `fn_body` 可被**行尾注释**满足（`normalize` 不剥行尾注释 + 首个匹配即返回） | `:237` + `:584-601` | 解析 `fn_body` 前先剥注释，或要求匹配不在注释内 |
| F-1 | 低中 | raw string 内含 `"` 使扫描器失步 ⇒ 提前归零/永不归零（Dev 已声明未处理） | `:288-395` | 在 `in_str` 态识别 `r#"` / `r##"` 前缀 |
| F-5 | 低 | 行尾注释不参与剥除 ⇒ 注释文本会进入配平窗口、影响 `called_idents` 扫描（假红来源） | `:234-243` | 若要兼顾，可只对 `called_idents` 的输入剥注释 |
| F-6 | 低 | `:234-236` 关于「剥注释会造假绿」的论证不成立（注释不执行）；保守结论可保留，理由需改写 | `:234-236` | 改写论证 |
| F-7 | 低 | Dev 报的 C3/C3b 红点（`:787`/`:799`）与实测（`:791`/`:803`）差 4 行；疑为引了 panic 文案行而非 `assert!` 行。R3b `:511` 实测有 `:511`(helper 不在 pool.rs) 与 `:521`(在 pool.rs) 两处，取决于构造 | — | 非阻断，供 Dev 对账 |
| C4 | 低 | `connect_sites` 的 `_` 前缀豁免（`:724`）仍在 —— Dev 已声明本需求不做且未登记 | `:724` | 维持未登记状态，别在后续需求里悄悄继承 |

---

## 我自己引入又清掉的问题（如实登记）

1. **`restore()` 缺陷（影响结论，必须记）**：`git checkout <commit> -- path` 会同时写**索引**，
   我原先的 `git checkout -- tests/ src/` 从索引恢复 ⇒ 老判据留在磁盘上。
   导致**整批标为「fixed」的 BB/BCb 其实跑的是旧判据**（靠「panic 行号是旧树的 :293」+ 脏标记发现）。
   修法：改用 `git reset --hard HEAD`，并在每次变异开始前断言 worktree 干净；**全表重跑后才采信**。
2. `compile_fail` 判定正则把 `error: test failed, to rerun pass` 误判为编译失败 ⇒ 改为排除该行 + 必须存在 `test result:` 行。
3. **BA/BB/BCb 初版把裸括号放在整行注释里** —— 而 `normalize` 会剥掉整行注释 ⇒ 这些变异**根本没进 `balanced_call`**。
   改造成行尾注释后才真正测到（这个坑与 Dev 在 `W5` 里踩的是同一个）。
4. 注入的闭包没加 `async` ⇒ E0728 编译失败 3 次。
5. `G_forbidden_comment` 被我自己的 `.await.unwrap()` 污染（多出 `unwrap` 这个 ident）⇒ 红在 `:511` 而非 `:489`，重造。
6. `G_whitelist_Some` 与 `pool.rs` 自身的 `Some(Duration::from_secs(..))` 撞名 ⇒ 改 `Option::Some`；随后签名不匹配 ⇒ 改为收 `Result`。
7. `C_ok_in/out` 的 `Ok(...)` 把 `Result` 又包了一层 ⇒ 改 `?`。

## 纪律偏离登记

本会话未暴露 `write`/`edit` 工具，按任务书允许的兜底改用**带引号 heredoc**（`<<'PY'` / `<<'EOF'`）
写临时脚本与本报告，未反复重试。
全程未设 `CARGO_TARGET_DIR`、未用 `--release`、未另建 probe crate、未删热 target 符号链接；
临时文件只在 `.review-workspace/verify-req043/`；口令只走环境变量、未写入本报告、未 echo 连接串；
共享本体库未做任何 DELETE/TRUNCATE；`fix-props/schema.json` 未触碰。
