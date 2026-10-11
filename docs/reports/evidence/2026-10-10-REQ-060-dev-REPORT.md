# REQ-060 · FDE 通道显示提案级 `extracted_by` —— Dev 报告

- **分支**：`feature/REQ-060`（worktree `feature-REQ-060/`，基线 `f90d537`）
- **commit**：`946bcd9`（主体） + `ca8cdc7`（端到端 + 修重复定义），**未 push**
- **状态**：全量 **637 passed / 0 failed / 1 ignored**（基线 619 + 新增 18）

---

## 结论先行

缺口成立且已补上。`review show` 现在**恰好一次**打印提案级归属，
四种状态各有明确表述，且**归属在类型层面就进不了 `premise_holds` 的判定范围** ——
AC-004 那个坑不是靠「记得别写」避开的，是**写不出来**。

**未发现本条 AC 把问题定位错。** 四条 AC 的事实前提逐条核实成立
（`ProposalRow` 有 `extracted_by` / `ProposalItemRow` 没有 / 原先 `review_view.rs` 零出现）。
下面第 6 节有**一处我认为 AC 可以更强的地方**（不是定位错，是覆盖不全），如实报。

---

## 1 · `review show` 现在的输出（实测原文）

`cargo test --test test_review_provenance -- --nocapture …print_all_four_states` 的真实输出，
**有 `extracted_by`** 的情形：

```
整体通过：否
提案级抽取归属：agent=mcode-agent · version=2026.10.07 · 第 1 轮（整份提案共用一份，不是逐条目的）
  [1] add_entity_type · 状态=Passed
      业务轨：客户 · biz=accepted
      建模轨：key=customer · 列映射：客户编号←customer_pk · 证据：t_customer.customer_no：主键列名一致 · model=accepted
  [2] add_entity_type · 状态=Passed
      业务轨：客户订阅 · biz=accepted
      建模轨：key=subscription · 列映射：客户编号←customer_id · model=accepted
  [3] add_link_type · 状态=Pending
      业务轨：客户 客户订阅 · biz=accepted
      建模轨：key=customer_has_subscription · 两端：customer→subscription · 基数：1:N · model=pending
```

归属行在**所有条目之前**、**恰好一行**，不进任何条目行。

## 2 · `None` 与形态可疑各输出什么

**`extracted_by` 为 `None`（SQL NULL = 人工修订，ADR-001）**
```
提案级抽取归属：无（人工修订，ADR-001：这份提案的 extracted_by 是 SQL NULL）（整份提案共用一份，不是逐条目的）
```
不打空、不省略、不打 `None` 字面量 —— 三条各有断言。

**形态可疑（`agent` 空串）**
```
提案级抽取归属：⚠ 形态可疑，不作为归属采信 —— `agent` 是空串；原始取值：{"agent":""}（整份提案共用一份，不是逐条目的）
```
**缺 `agent`**
```
提案级抽取归属：⚠ 形态可疑，不作为归属采信 —— 缺 `agent` 栏（没有它就无法回答「谁抽的」）；原始取值：{"round":1,"version":"1.0"}（整份提案共用一份，不是逐条目的）
```

六种可疑形态（空串 / 纯空白 / 缺 `agent` / `agent` 非字符串 / 不是对象 / JSON null）
**各自的原因互不相同** —— 有断言（`reasons.len() == cases.len()`），
确保「可疑」不是一句套话。

### ⚠️ 我加了第四态：`NotProvided`（AC 没点名）

`cli.rs` 走三参入口（有这一栏）；但**两参**入口（老调用点 / 纯逻辑测试）压根没收归属。
把两者都显示成「人工修订」= **给出假安心**（「有人审过」vs「我不知道」，含义相反）。
所以分成两个状态：

```
提案级抽取归属：未提供（这条路径没有接提案行，无法判断谁抽的 —— **不是**「人工修订」）（整份提案共用一份，不是逐条目的）
```

## 3 · AC-004 的判别力证据

### 结构性保证（设计层）

归属挂在 **`ReviewTrackView` 顶层**，**不进 `ModelFacing`**。
`sourced_fields()` 的接收者是 `&ModelFacing`，`audit_fde_channel()` 只逐条读它
⇒ 归属与 `premise_holds` **在类型层面无交集**。

### 独立核实：`premise_holds` 没有因为显示归属而变真

**口径**：造一份**连 `key` 都没有**的条目（`sourced_fields()` 必然全空），
在「无归属」与「有归属」两种输入下问 `premise_holds` 与 `hits`，
**两个取值必须逐 bit 相同**。纯逻辑与端到端各一条。

实测输出（无归属 / 有归属）：
`premise_holds == false`，`hits == []`，**完全一致**。

### 变异测试：3 个变异全部被杀

| # | 变异 | 被谁抓 |
|---|---|---|
| 1b | **AC-004 的真实陷阱**：无 payload 建模字段时，仅因渲染了归属就算命中 | `premise_holds_is_bit_identical…` → 报 `前提基线不成立 … ["item[1].provenance"]` |
| 2 | **AC-002 误导变体**：逐条目重复打印归属 | **3 条**独立用例 |
| 3 | **AC-003**：可疑形态照打成正常归属行 | **2 条**用例 |

⚠️ 变异 1b 是关键：AC 点名的那个失败模式**确实**被 `premise_holds` 那条判据直接抓住。

### 回归项

- 客户视图建模术语守门：`src/domain/view.rs` **零差异**；
  另有断言「业务轨那一侧 `scan_modeling_terms` 仍为空」。
- `audit_fde_channel` 判别力：未改一行，**未改回读词表**。
- 两轨分界：三种归属状态（无 / 有 / 可疑）下 `verify_track_separation` 均零违规。
- `test_humanize_still_refuses_unknown_columns`（在 `src/domain/view.rs` 的
  `#[cfg(test)]` 里，**不在** `tests/`）：函数体与基线 **diff 完全一致**。

## 4 · 全量数字与产物 sha

| 项 | 值 | 口径 |
|---|---|---|
| **全量（改后）** | **637 passed / 0 failed / 1 ignored**，**717s**（11 分 57 秒） | `cargo test --no-fail-fast`；只数 `^test result: ok\. N` 行后 `awk` 求和，共 **46** 条（基线 45 + 新增 1 个测试文件） |
| **全量（真基线）** | **619 passed / 0 failed / 1 ignored** | 同口径，**45** 条 `test result:` 行 |
| 差值 | 619 + **18** = **637** ✅ | 新增 = 纯逻辑 **14** + 端到端 **4** |

**产物 sha（与基线逐字节一致）**

| 产物 | sha256 | 对照 |
|---|---|---|
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | ✅ 一致 |
| `proposal.json`（`--project extracted`） | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | ✅ 一致 |

命令（`ontok-propose`，**不传 `--names`**，`--stats` 自带路径）：
```
cargo run -q --bin ontok-propose -- \
  --schema /home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json \
  --out <W>/stats.json --stats <W>/stats.json
cargo run -q --bin ontok-propose -- \
  --schema <同上> --out <W>/proposal.json --stats <W>/stats.json --project extracted
```

## 5 · 发现但未修 / 自己引入又清掉

**发现但未修**

1. **`extract/propose.rs` 的 `ProposalProvenance.failed_groups` 是
   `Vec<serde_json::Value>`，而 `snapshot::ExtractedBy` 是 `Vec<String>`。**
   本条渲染层对元素用 `as_str()` + `to_string()` 兜底（非字符串元素原样打），
   **不静默丢**。但这两种形状不一致是 REQ-022 改名时留下的既有债，
   **不在本条 scope**，未动。
2. **`ProposalProvenance.source`（勘查留痕，`Option<String>`）未进本投影。**
   AC 只要求显示「谁抽的」。`source` 与 `snapshot::ExtractedBy.source`
   **含义不同**（前者是 schema 勘查留痕，后者是来源通道），
   混进来会重演 ADR-003 那次误读。未动。
3. **两参入口 `project_review_track` 现在恒为 `NotProvided`。**
   `tests/test_review_view.rs` 里 `fde_view()` 走的正是它 ⇒ 那份 fixture 的输出
   会说「未提供」。这是**如实**的（那个 fixture 确实没有提案行），
   但若将来有别的两参调用点**有**归属却被显示成「未提供」，那是**安全侧**的失败
   （FDE 保持警惕），不是危险侧。

**自己引入又清掉**（详见工作笔记 `NOTES.md`）

4. 第一轮「基线」全量被自己的半成品污染（doc-test 编译进重复定义报错）⇒ 该数字作废，重跑真基线。
5. `stash` 往返丢过一次 `review_view.rs`（`git checkout -- src/` 覆盖了 pop 结果）⇒ 从 commit 恢复。
6. 修重复定义时误删 `passed: status.passed` ⇒ `E0063`。
7. 模块文档打进一个乱码字符（`于�`）。
8. 文档先写「三态」，实现是四态。
9. **我自己写的一条断言方向反了**：曾断言「可疑形态 ⇒ 判据报违规」。
   实际相反 —— **正确标注了可疑时判据应当放行**（判据报的是「呈现没做到位」）。
   已改成双向断言：未变异必须放行 + 变异「照打可疑」必须被抓。
10. **在全量 `cargo test` 运行期间改了源码做变异** ⇒ 那次全量作废，重跑一次干净的。

## 6 · 给 BA 的反馈（本条 AC 未定位错，但有一处可更强）

AC-001 写的是「`extracted_by` 为 `None` ⇒ 必须有明确的『无/人工修订』表述」。
这条**把两种语义合并了**：

- 库里是 **SQL NULL** = 人工修订（ADR-001）—— 这是**有信息的**；
- 调用方**压根没提供**这一栏 = 「我不知道」—— 这是**没信息的**。

按 AC 字面实现（两种都显示「人工修订」）会在第二种情况下**给出假安心**，
而这正是 REQ-056 那句「猜错比不给更危险」的另一种形态。
我拆成了 `HumanRevised` / `NotProvided` 两个状态并各有断言。
**建议后续 AC 把这类「`None` 其实是好几种东西」的地方单列。**

另：AC-003 列的可疑形态是「空串 / 只有空白 / 缺 `agent`」，
我另外补了 **`agent` 不是字符串**、**整体不是对象**、**JSON null** 三种 ——
它们同样是「写了但写得不对」，且 JSON null 特别值得单列：
它**不是** SQL NULL，混进「人工修订」同样是假安心。

## 7 · 环境类说明（与代码失败分开）

- **代码失败**：0。全量 637/0/1，三条既有 review 测试文件零回归。
- **环境类**：本机无 `psql` / `psycopg` ⇒ `SELECT 1` p50 **未测**（环境不具备，非本条范围）。
- 共享本体库**全程只读**，未执行任何 `DELETE` / `TRUNCATE`。
- 口令只经环境变量，未 `echo` 连接串，未写入任何文件 / 日志 / commit。
- 仓库无 remote，**未 push**；未 squash、未 force、未改历史。

## 8 · 临时文件

全部在 `.review-workspace/req060/`（`NOTES.md` / 三份全量日志 / 产物 / 实测输出 / 变异备份）。
**未用 `/tmp`、未用 home。** 未执行 `rm`。
