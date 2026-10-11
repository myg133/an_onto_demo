# 验收标准：REQ-057 修 `audit_fde_channel` 恒真（F-A）+ `type_key` 回退（F-D）

> 依据 REQ-056 验收（QA 判 **FAIL**，一个必须修的阻断项）写成。基线：`06ae7c9`（develop）。
>
> **⚠️ 2026-10-09 BA 补登记：Dev 改了一条 REQ-056 的既有断言（已由 QA 裁决「接受」）**
>
> ```diff
> -        !link.model.type_key.contains("客户"),
> -        "建模轨的 type_key 不该是展示名：{}",
> +        // 既然 `type_key` 现在**只**来自 payload，那就直接断言它等于 payload 里的 key。
> +        link.model.type_key.as_deref(),
> +        "建模轨的 type_key 必须逐字等于 payload.key，不能是展示名：{:?}",
> ```
>
> **为什么必要**：本文件开篇写「**只修这两条**」，而 Dev 实际动了**上一轮需求的测试**。
> 当时 `BA/demands/REQ-057/` 下**没有**这条登记 —— 属 BA 流程漏记，QA 复核时指出。
>
> **QA 裁决：接受**，且给出的是**集合包含论证**（强于抽样）：
> `Pass(新) ⟹ type_key == "customer_has_subscription" ⟹ ¬contains("客户") ⟹ Pass(旧)`
> ⇒ `Pass(新) ⊂ Pass(旧)` ⇒ **不存在**「旧抓新放」的输入。
> **实证 51 样本**：旧过新挂 **6** 个、**新过旧挂 0 个** ⇒ **纯收紧成立**。
> 旧写法只检查「不含『客户』两个字」，而 BYPASS_E 的展示名正是「主键服务中心」—— 不含「客户」，能蒙混过关。
>
> **流程教训（BA 记）**：子 agent 若被 AC 点名要改**别的需求的产物**（含其验收标准里的断言），
> **AC 必须显式点名**「你可以改 X 的第 N 条断言」并写清改的理由；
> 否则子 agent 要么不敢改（漏做），要么改了而 BA 事后才知道（本案）。

---

## ⚠️ QA 判 FAIL 的理由（一句话）

> **恰恰是那个最聪明的设计，判据没落在它声称的地方。**

REQ-056 用守门器自己验「FDE 通道豁免」的前提（`premise_holds = !hits.is_empty()`）——
设计很聪明，**但它对任何非空提案恒为真**，证明不了它想证明的事。

---

## F-A（阻断）· 判据恒真

**根因（QA 与 BA 各自独立核实）**：

`src/domain/review_view.rs` 渲染时**对每个条目无条件打** `key=`：

```rust
let mut parts: Vec<String> = vec![format!("key={}", it.model.type_key)];
```

而 `key` ∈ `MODELING_TERMS` ⇒ `scan_modeling_terms` **必然命中** ⇒
`premise_holds = !hits.is_empty()` **对任何非空提案都是 `true`**。

**QA 的探针 BYPASS_E**：建模字段**一个都没打**（`type_key` 空、无 `columns`/`evidence`/
`cardinality`/两端），展示名写成「主键服务中心」：

```
premise_holds = true    hits = ["主键", "key"]
```

⇒ `premise_holds` 实际测的是「**提案是否非空**」，不是「**建模字段是否真打出来了**」。
⇒ `cli.rs` 那句 `⚠ FDE 通道自检：这份建模投影里没扫到建模术语 —— 建模字段可能没打出来`
**永远不会触发**。

## F-D（QA 独立发现，Dev 未提）· `type_key` 回退冒充 key

`src/domain/review.rs`（**红线文件，本需求不许改**）的 `type_key()`：

```rust
self.payload.get("key").and_then(|v| v.as_str())...   // 取不到时回退到 target
```

⇒ payload 缺 `key` 时，**建模轨会把展示名当 key 显示出来**。

---

## BA 的判断：F-A 与 F-D 是**同一个根因的两面**

建模轨字段的来源是「**payload；取不到就回退或打空**」，
而判据是「**看输出里有没有建模术语**」。
⇒ **回退值和空值都被判成「有」** —— 所以**不能只在判据上打补丁**。

---

## AC-001 · 判据必须真的落在「字段有没有打出来」上

- 预期结果：`premise_holds` 反映的是
  「**建模字段确实来自 payload 且非空**」，**不是**「输出里出现过建模术语」
- 类型：功能
- **判别力自证（必做，各给**二进制名 + 运行条数**）**：
  1. 复现 QA 的 **BYPASS_E**（建模字段全空）⇒ **必须** `premise_holds == false`
     （现状是 `true` ⇒ **本条不通过就是没修好**）
  2. 正常条目（有 `key`/证据/列映射）⇒ 仍为 `true`（**别修过头**）
  3. 构造「**只有 `key=` 空串、其余建模字段全无**」⇒ **必须** false
- ⚠️ **编译失败不算变异执行**

> ⚠️ **QA 已实测一个反向陷阱**：词表里**新增一个词**会让
> `premise_holds` 更容易成立（**词表越宽、自检越松**），
> 而客户视图侧同时**更严**。**共用词表带来的是一致性，不是正确性。**
> 你若靠「输出里有没有词」这条路径修，**正好落进这个陷阱**。

## AC-002 · `type_key` 回退不能冒充 key

- 预期结果：payload 缺 `key` 时，建模轨**不把展示名当 key 显示**。
  要么显式标注「（无 key）」、要么省略该字段、要么另走一路 —— **由你判断**
- ⚠️ **红线**：`review.rs` 是红线文件**不许改** ⇒ 修法必须在 `review_view.rs` 侧
- 类型：功能
- **判别力自证**：造一个 payload **无 `key`** 的条目 ⇒ 输出里**不得**出现
  「展示名被当 key」的形态；给一条**逐字断言**（不要只断言「有输出」）

## AC-003 · Bypass 覆盖补齐

QA 独立构造出 **BYPASS_C（两轨内容重复）现有判据完全不抓**。

- 预期结果：至少补上 **BYPASS_A / B / C** 三条的判据
  （A：建模字段塞进业务轨行；B：省略 `key` 只留 `evidence`；C：两轨内容重复）
- 类型：测试有效性
- ⚠️ 若你判断某条**不该**被抓住，**说清理由** —— 别默默放过

## AC-004 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**596 passed / 0 failed / 1 ignored**（本需求会新增用例，**请写清最终条数**）
- **基线对照**：REQ-056 那轮 QA 补跑过 —— `d3ebea8` = **584/0/1**，
  HEAD = **596/0/1**（+12 = `test_review_view` 7 + `test_review_show_e2e` 5）。
  **你这轮基线是 `06ae7c9` = 596/0/1**
- 红线：`src/domain/view.rs`、`src/domain/review.rs`、`tests/drop_rate.rs`、
  `tests/test_fk_naming.rs` **零改动**；
  **客户视图的建模术语守门一条不许动**；
  `test_humanize_still_refuses_unknown_columns` 一条断言都不许放宽；
  **无测试被删除或 `#[ignore]`**
- 产物 sha **不变**：
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
  `relative URL without a base` = **URL 取残缺**，不是环境坏了
- 共享本体库**只读**；**禁 DELETE / TRUNCATE / 清空表**
- 本机**没有 `psql`、没有 `psycopg`** ⇒ **测不了 `SELECT 1` p50**
- 报告把「代码失败」与「环境类失败」**分开写**；复现率随负载浮动，**不声称稳定值**
- 口令只走环境变量，**绝不写进代码、文件、日志、报告或 commit**；
  ⚠️ **不要 `echo` 连接串**

## 红线清单

| 文件 / 约束 | 说明 |
| --- | --- |
| `src/domain/view.rs` | **禁改**（建模术语守门所在处） |
| `src/domain/review.rs` | **禁改**（F-D 的根源在它里面，但**不许碰**） |
| `tests/drop_rate.rs` | **禁改**（⚠️ 路径在 `tests/` 下，**不在** `src/domain/` —— BA 本文件初稿写错过一次，QA 复核时纠正） |
| `tests/test_fk_naming.rs` | **禁改**（⚠️ 同上，在 `tests/` 下） |
| **客户视图的建模术语守门** | **一条不许动** |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| 任何测试 | **不许删除、不许 `#[ignore]`** |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖** |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target
- ⚠️ **不要与另一个全量测试并发跑**
- **求和不要用 `paste -sd+`**
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- **核对文件用裸 `grep`**；**别信任何行号**（本会话已 4 处行号引用失效）
- 临时文件只放 `.review-workspace/req057/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
- commit 格式：`[Dev] REQ-057 {描述} (关联: REQ-057) [session: ba-archlinux-80180-20261007-231834]`

## 交付

提交到 `feature/REQ-057`（不 push），报告里给出：
1. **F-A 你怎么修的** —— 为什么不落进「词表越宽、自检越松」那个陷阱
2. **BYPASS_E 修后实测**（必须 `premise_holds == false`）
3. F-D 怎么处理的（`review.rs` 不许动的前提下）
4. BYPASS_A/B/C 三条的新判据，或「不该抓」的理由
5. 全量数字与耗时、产物 sha
6. 发现但未修的问题 / 你自己引入又清掉的问题（如实报）

诚实优先于好看。**如果你判断 F-A 的正确修法是「把 `audit_fde_channel` 降级为注释、
不再声称它是判据」，直说** —— QA 明确给过这个选项，比硬撑一个不成立的判据好。