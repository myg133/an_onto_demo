# REQ-057 · F-A / F-D 修法决策

工作区：`/home/myg133/Projects/Anchnet/Ontology/feature-REQ-057`
分支：`feature/REQ-057` · 基线：`06ae7c9`

## 决策：不把 `audit_fde_channel` 降级为注释

QA 给过「降级为注释」这个选项。**不采纳**，理由：AC-001 明文要求
`premise_holds` 对 BYPASS_E 判 `false`。降级 = 删掉这个布尔值 = 违反 AC-001。
正确修法是**把判据从「文本扫描」换成「结构化溯源」**。

## F-A 根因与修法

根因：判据（`scan_modeling_terms(&str)`）和数据来源（payload）不是同一个东西。
- 数据源：payload 取不到就**回退**（`type_key()` 回退到 target）或**打空**；
- 判据：渲染文本里**有没有建模术语**。
⇒ 回退值和空值都被判成「有」。

修法：`audit_fde_channel` 签名 `(&str)` → `(&ReviewTrackView)`。
`premise_holds` 改为：∃ 条目，其 `ModelFacing` 里**至少有一栏既来自 payload 又非空**。

### 为什么不会落进「词表越宽、自检越松」陷阱

陷阱的机制是：判据依赖 `MODELING_TERMS`，词表越宽 → `hits` 越容易非空 → 豁免越容易成立。
新判据**完全不调用 `scan_modeling_terms`**，也不读渲染文本。
它读的是 `ModelFacing` 的字段溯源。往词表里加「客户」：
- 客户视图侧：仍然**更严**（命中即拒）；
- FDE 侧 `premise_holds`：**一个 bit 都不会变**。
⇒ 两个方向不再共用同一条路径，「加词 ⇒ 一边更严一边更松」的耦合被**结构性切断**。

## F-D 修法（`review.rs` 零改动）

`ModelFacing::type_key: String` → `Option<String>`，
取值**只**来自 `payload["key"]` 且非空。**不再回退到 `it.target`**。
渲染时 `None` → 打 `key=（无 key）`，显式标注缺失，而不是拿展示名冒充。

## 分工（两个判据各管一件事，不重复报）

| 判据 | 回答的问题 | 数据源 |
| --- | --- | --- |
| `audit_fde_channel(&view)` | 「这份投影里到底有没有真建模数据」 | `ModelFacing` 字段溯源 |
| `verify_track_separation(&view, &text)` | 「两轨渲染有没有串轨/重复/漏字段」 | `BizFacing`/`ModelFacing` vs 渲染行 |

## BYPASS 三条的新判据

- **A（建模字段塞进业务轨行）**：`verify_track_separation` 规则 ② ——
  业务轨行去掉前缀后按 ` · ` 切分，必须**恰好**是
  `[display_target, "biz=<status>"]` 两个 token。多一个 token 就是违规。
  **不依赖任何词表**（不是 marker 列表）。
- **B（省略 key 只留 evidence）**：F-D 修法直接命中 —— 渲染 `key=（无 key）`，
  展示名绝不冒充 key。且 `audit_fde_channel` 的 `hits` 会列出
  `item[N].evidence` 而**不含** `item[N].key`，缺口可见。
  注意：`premise_holds` 此时**仍为 true**（evidence 确实是 payload 来源且非空的建模栏）——
  这是正确语义，前提判据**不负责**判断条目完整度，那是 `verify_track_separation` 的活。
- **C（两轨内容重复）**：规则 ①（任一行不得同时带两个轨前缀）
  + 规则 ③（建模轨行不得等于业务轨行）
  + 规则 ④（每个 payload 来源的非空字段，其渲染标记必须出现在建模轨行上）。
  三条任一即可抓住重复 / 漏打。

## 变更面（已 grep 确认封闭）

`src/domain/review_view.rs` · `src/cli.rs` · `tests/test_review_view.rs` · `tests/test_review_show_e2e.rs`

红线文件零改动：`view.rs` / `review.rs` / `drop_rate.rs` / `test_fk_naming.rs`。