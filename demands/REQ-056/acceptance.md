# 验收标准：REQ-056 `review show` 展示建模字段

> 依据 REQ-055 验收（Dev 主动报 P1、QA 判定成立且比 Dev 说的更硬）写成。
> 基线：`d3ebea8`（develop）。
>
> **这是规格 §8.3 的真正落点。** REQ-055 做的是「审已落库快照的建模质量」，
> 本需求做的是「**裁决提案时**看到建模质量」—— 两者不是同一件事。

---

## 背景：现状是什么

规格 §8.3 拆双轨的理由：**客户没能力判断建模质量**（主键、基数、规范化），
让他审只会随便点「通过」；建模轨交给 FDE。

**但现状下 FDE 在 apply 前只能闭眼点通过。**

`src/cli.rs` 的 `ReviewCmd::Show` 分支（**按内容 grep 定位**）：

```rust
let items = proposals.items(p).await?;
let views = PgProposalStore::to_item_views(&items);   // ← 建模字段在这里
let st = summarize(&views, &baseline);                 // ← views 只被这个用

for (r, v) in items.iter().zip(st.verdicts.iter()) {  // ← 循环却用 items
    println!("  [{}] {:<18} {:<12} biz={:<8} model={:<8} {:?}",
        r.seq, r.target_key, r.kind, r.biz_status, r.model_status, v.state);
    //                                        ↑ payload 一个字没打
}
```

**`views` 已经算好了，只是没被打印用。** 循环遍历的是 `items`（存储层原始行），
打印 `seq`/`target_key`/`kind`/两轨状态/`state` —— **没有任何建模信息**。

### 时序事实（QA 挖出，BA 与 Dev 都没点明）

- `cli.rs` 的 apply 分支有 `!st.passed` 门禁，`review.rs` 要求每条目**两轨都 `Passed`**
  ⇒ **FDE 裁决发生在提案落库之前**
- ⇒ REQ-055 那个「读已落库快照」的建模轨投影，**在裁决时能给的只是 base 版本**
  —— 不是同一个对象

### 但数据就在裁决对象上

`ItemView` 的注释写着「**条目的完整候选定义（含 evidence）**」，
`db/proposal.rs` 的 `to_item_views()` 确实从 `r.payload` 构造它；
`EntityPayload` / `LinkPayload` 带的**恰好就是建模轨要的那几栏**
（`key` / `evidence` / `column` / `cardinality` / `from_type` / `to_type`）。

⇒ **缺的只是呈现。** 这是本需求成立的全部依据。

---

## AC-001 · `review show` 展示提案条目的建模字段

- 预期结果：FDE 在 `review show` 里**看得到**该提案条目的建模信息
  （至少：类型 key、证据、列映射、关系基数、两端类型）
- 类型：功能
- **判别力自证（必做，两条各给**二进制名 + 运行条数**）**：
  - 造一条**含建模术语**的提案条目 ⇒ `review show` 输出**必须含**它们
  - **同一份提案**在**业务轨**的呈现上**不得**因此被污染
    （见 AC-002 的「两轨分界」要求）

## AC-002 · 建模术语不因此进入客户视图

**这是本需求唯一有真正风险的地方。** REQ-055 最硬的约束是
「建模轨绝不混进客户视图」，本需求要把建模术语引到 CLI 输出，**必须论证没破坏那条**。

- 预期结果：
  - `review show` 是 **FDE 通道**，其输出**不经**建模术语黑名单守门
    （守门只扫客户视图那条路径）
  - **独立核实** `cli.rs` 里客户视图的输出路径**没有**因为本改动而多出建模术语
- **判别力自证**：构造一个「建模字段同时出现在 `review show` 与客户视图输出」的场景，
  看**守门还拦不拦得住客户视图那一侧**
- ⚠️ **本会话已连续 4 次栽在「诊断文案/输出过度声称」上**，
  若你决定「FDE 通道不受守门约束」，**必须在代码里留下判据**，不能只靠约定

## AC-003 · 两轨分界不糊

- 预期结果：输出里**业务轨该看的**（类型名、命名）与**建模轨该看的**（key、evidence、cardinality）
  **可分辨**，不混成一坨
- **判别力自证**：变异「把两轨字段混在一起打」⇒ 必须被某条判据抓住
- 类型：架构

## AC-004 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**584 passed / 0 failed / 1 ignored**（本需求若新增判据则相应增加，
  **请在报告里写清最终条数并知会 BA**）
- **建议同会话基线对照**（基线 `d3ebea8` = **584/0/1**）
- 红线：`src/domain/view.rs`、`tests/drop_rate.rs`、`tests/test_fk_naming.rs` **零改动** ——
  ⚠️ 建模术语黑名单守门**一条不许动**；
  `test_humanize_still_refuses_unknown_columns`（在 `view.rs` 内）一条断言都不许放宽；
  **无测试被删除或 `#[ignore]`**
- 产物 sha **不变**（本需求只改审核呈现，不碰抽取逻辑）：
  - `stats.json` = `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba`
  - `proposal.json`（`--project extracted`）= `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635`
  - **输入就是 `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`**
  - 命令实属 **`ontok-propose`**；⚠️ **`--stats` 必须自带路径参数**；**不传 `--names`**

---

## 环境事实

- ⚠️ `.env` **在仓库根**，各 worktree 内**没有**。库名 `notol` **必须替换成 `ontol`**
  ```bash
  export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' \
    /home/myg133/Projects/Anchnet/Ontology/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"
  ```
  `relative URL without a base` = **URL 取残缺**，不是环境坏了（本会话已两轮在这里栽过）
- 共享本体库**只读**（仅 `information_schema` / `pg_catalog`，零业务数据行）；
  **禁 DELETE / TRUNCATE / 清空表**
- 本机**没有 `psql`、没有 `psycopg`** ⇒ **测不了 `SELECT 1` p50**
- 报告把「代码失败」与「环境类失败」**分开写**；复现率随负载浮动，**不声称稳定值**
  （已知 `Connection reset by peer` 是环境类，隔离复跑通常过）
- 口令只走环境变量，**绝不写进代码、文件、日志、报告或 commit**；
  ⚠️ **不要 `echo` 连接串**（本会话已有 Dev 与 QA 各踩过一次口令回显的坑）

## 红线清单

| 文件 / 约束 | 说明 |
| --- | --- |
| `src/domain/view.rs` | **禁改**（建模术语黑名单守门所在处） |
| `tests/drop_rate.rs` | **禁改** |
| `tests/test_fk_naming.rs` | **禁改** |
| **客户视图的建模术语守门** | **一条不许动**（REQ-055 的核心不变量） |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| 任何测试 | **不许删除、不许 `#[ignore]`** |
| **两轨分派逻辑**（`review.rs` 的 `apply_decision` 等） | **不许改**（本需求只改呈现） |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖** |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target（本会话连踩两次）
- ⚠️ **不要与另一个全量测试并发跑**（本会话已两次互相拖垮）
- **求和不要用 `paste -sd+`**（逐行输出 ⇒ 内容重复即精确翻倍）
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- **核对文件用裸 `grep`**；**别信任何行号**（本会话已 4 处行号引用失效）
- 临时文件只放 `.review-workspace/req056/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
- commit 格式：`[Dev] REQ-056 {描述} (关联: REQ-056) [session: ba-archlinux-80180-20261007-231834]`

## 交付

提交到 `feature/REQ-056`（不 push），报告里给出：
1. `review show` 现在的输出长什么样（**贴实测原文**）
2. **AC-001 / AC-002 / AC-003 的判别力证据**，各给二进制名 + 运行条数
3. 「FDE 通道不受守门约束」这条**怎么落成判据**（不是只靠约定）
4. 全量数字与耗时、产物 sha
5. 发现但未修的问题 / 你自己引入又清掉的问题（如实报）

诚实优先于好看。**如果你判断「评审呈现该走 HTML/MCP 而不是 CLI 文本」，
或「该先把 REQ-035 的 RP-2（`extracted_by` 归属）一起修」，直说** ——
你上一轮主动报的 P1 就是这么发现的，继续这样报。