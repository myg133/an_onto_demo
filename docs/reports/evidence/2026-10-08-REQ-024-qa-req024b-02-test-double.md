# 替身改动判定

## 决定性证据：守卫是**本批新增**的
`git show 563f26f:src/agent/assemble.rs` —— 父提交**没有 `contract_gaps` 这个函数**，
`links`/`evidence` 判据在父提交里**完全不存在**（只有 rebuild 的 links 合并与 AC-007 retain）。

## 1. 是否掩盖真实缺陷？—— 否
`src/extract/propose.rs:538-570` `links_for_table`：规则管线里 `infer_link(c, known_tables)`
**只在 known_tables 内确认目标存在才产出关系** ⇒ **规则管线永远不会产出悬空关系**，
且**永远填** `{table, column, claim}` 三件证据（propose.rs:561-565）。
⇒ 「幽灵关系 + 无证据」在规则路径里**不存在**，原夹具的 `Vec::new()` 是**惰性/潜伏**缺口：
改前无守卫 ⇒ 无害；且该链接在 `assemble.rs:281` 的 AC-007 `links.retain` 就被丢弃，
**根本走不到 `link_item`**，空证据从未被搬运。
所以：补证据 = **修正一个本就不该为空的夹具**（把潜伏缺口清掉），
不是「为了让守卫通过而改测试」。真实缺陷（证据缺失）由 `EvidenceLessLinkAgent` 继续覆盖。

## 2. AC-007 判据强度 —— **不变**（既未变强也未变弱）
- 守卫在 orchestrate.rs:640-674 逐组跑；AC-007 在 `rebuild_outcome`（assemble.rs:278-282）阶段 3+4 跑。
- 改动后 `IncompleteRebuildAgent{missing:"", conflict_link:true}` 的 `contract_gaps` 返回**空**
  （types 非空 / rejected_comments=Some(0) / origin=Some / nullable=Some / evidence 现非空）
  ⇒ 守卫放行 ⇒ 仍然走到 AC-007，断言 `links_dangling == 3` 与「悬空不进 items」**一字未动**。
- 反证：若把 evidence 改回空，守卫会 bail，`test:855` 的
  `.expect("AC-007 自检不是 Err，是计入统计")` 会 **panic** ⇒ 改动是**必需**的。
- 边际增强：现在额外证明「守卫不误杀带证据的悬空关系」（不过度严格）。

## 3. `EvidenceLessLinkAgent` 是否纯粹？—— **是**（按代码逐项核）
`contract_gaps`(assemble.rs:105-166) 五个判据对该替身：types 非空 ✓ / rejected_comments=Some(0) ✓ /
origin=Some(Comment) ✓ / nullable=Some(false) ✓ / links[].evidence 为空 ← **唯一缺口**。
⚠️ 两点保留（均非缺陷）：
- 注释说「端点都在本组作用域内」**不准确**：to_type=`t_customer` 属**另一组**；
  只是阶段 3 合并后存在 ⇒ 不悬空，结论对、理由错。
- 该测试只断言 `msg.contains("evidence")`，**未断言「只有这一个缺口」**；
  若将来有人给替身加第二个缺口，这条测试仍会通过 ⇒ 纯度靠夹具而非断言钉住。

## 4. 其他替身是否被波及？
`git diff 563f26f 40b755d -- tests/` 仅 3 个文件，逐个核（见 03）。
