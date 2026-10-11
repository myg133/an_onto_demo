# REQ-057 · 变异执行记录（判别力自证）

基线 `06ae7c9`。全部变异**已回滚**，回滚后以 sha256 + `git status` 双重确认。
**编译失败的一律不算执行** —— 下列三次变异均编译通过并真实跑了测试。

口径：`cargo test --test test_review_view`（**纯逻辑二进制**，零 I/O，不连库）。
该二进制本轮用例数：**19**（基线 `06ae7c9` 为 7，+12）。

---

## M1 · 把判据还原成 REQ-056 的「扫文本」 —— 验 F-A 的判别力

变异内容：`audit_fde_channel` 内部改成
`scan_modeling_terms(&render_review_text(view))`，`premise_holds = !hits.is_empty()`。
（改成收 `&ReviewTrackView` 以便**编译通过** —— 编译失败不算执行。）

结果：**13 passed / 6 failed**

失败的 6 条（全部是 `premise_holds` 相关）：

| 用例 | 为什么该挂 |
| --- | --- |
| `bypass_e_premise_false_even_when_terms_present_in_text` | **BYPASS_E 本体** |
| `empty_key_alone_does_not_hold_premise` | 空串 `key` |
| `premise_still_holds_for_normal_items` | `hits` 形态（逐条指名） |
| `hits_are_field_provenance_not_wordlist` | `hits` 形态 |
| `fde_exemption_is_conditional_on_a_checkable_property` | 反例（建模栏全空） |
| `bypass_b_evidence_only_never_shows_display_name_as_key` | `hits` 含 `item[1].evidence` |

⇒ **存活变异 0 条。** F-A 的新判据被这 6 条共同钉住。

---

## M2 / M2B · 把 `type_key` 回退还原 —— 验 F-D 的判别力

M2 代理版（回退到 `payload.display_name`）与
**M2B 忠实版**（投影处直接写 `Some(it.type_key())`，即 `review.rs` 里那条真路径）
—— 两者结果**完全相同**：

结果（两者均）：**15 passed / 4 failed**

失败的 4 条：

| 用例 | 为什么该挂 |
| --- | --- |
| `missing_key_never_renders_display_name_as_key` | **F-D 本体** |
| `bypass_b_evidence_only_never_shows_display_name_as_key` | BYPASS_B |
| `bypass_e_premise_false_even_when_terms_present_in_text` | 回退值把 premise 撑起来 |
| `fde_exemption_is_conditional_on_a_checkable_property` | 同上 |

⇒ **存活变异 0 条。**

### 一条**如实报告**的例外

`blank_key_does_not_fall_back_to_display_name` 在 M2B 下**仍然通过**。
原因：原始 `type_key()` 对 `key = "  "` 返回的是 `Some("  ")`（不 trim），
所以**根本没走回退分支**。该用例守的是「key 存在但全是空白」——
这是我在 F-D 之上**额外加的硬化**（`payload_key` 里 trim + 过滤空串），
不是 F-D 原缺陷本身。故 M2B 抓不到它，**不构成存活变异**。

---

## M3 · 词表加词实验 —— 验「词表越宽、自检越松」陷阱被切断

变异内容：`MODELING_TERMS_CJK` 加一个「**客户**」
（`src/domain/view.rs` —— **红线文件**，仅为取证临时改，事后按 sha256 复原）。

结果：**15 passed / 4 failed**

失败的 4 条，**全部在客户视图那一侧**：

- `biz_facing_not_polluted_by_modeling_fields`
- `same_data_opposite_verdicts_on_two_channels`
- `fde_exemption_is_conditional_on_a_checkable_property`（失败于**客户侧**那一断言）
- `gate_still_blocks_customer_view_when_terms_injected`

⇒ 与 QA 实测**同号**：加词让**客户侧更严**（「客户」是核心业务词 ⇒ 误杀）。
这部分是**没动过的红线守门**，行为本就该如此。

### ⭐ 关键结论

**所有 `premise_holds` 断言在加词后依然全绿**
（`bypass_e_*` / `empty_key_*` / `premise_still_holds_*` / `hits_*` / `bypass_b_*` 均 passed）。

⇒ 加词对 FDE 侧 `premise_holds` **零影响**。
QA 观测到的「加词 ⇒ FDE 侧自检更松」这一半**已被结构性消除**：
新判据**不读词表**，词表怎么变都与它无关。

---

## 回滚确认

| 文件 | 回滚后 sha256 | 与备份一致 | `git diff` |
| --- | --- | --- | --- |
| `src/domain/view.rs` | `c0c2df2c…50af56e6` | ✅ | 空 |
| `src/domain/review_view.rs` | `30c29f46…166a175d3` | ✅ | （即最终交付态） |

`grep -c MUTATION src/domain/review_view.rs` = **0**
`git status --short` = 仅 4 个预期文件，无红线文件。