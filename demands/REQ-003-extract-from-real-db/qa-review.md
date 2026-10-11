# REQ-003 独立审核报告

## 0. 审核元信息

| 项 | 值 |
| --- | --- |
| 被审对象 | `/home/myg133/Projects/Anchnet/Ontology/feature-REQ-003/`，分支 `feature/REQ-003`，HEAD `7d12548` |
| 变更范围 | 12 文件 / +1301 −8（`git diff --stat develop...HEAD`） |
| 审核者 | 独立审核 Agent（未参与开发）。全部结论基于自建实验，未采信作者结论 |
| 变异实验 | 只在副本 `/home/myg133/req003-review/`（`git archive HEAD` 导出）进行 |
| 原 worktree | **全程零修改**（收尾 `git status --short` 为空；`diff -r src tests` 与副本逐字节一致） |
| git 写操作 | 无（仅 `log` / `diff` / `show` / `archive` / `status`） |
| 口令 | 全程只经 `ONTOK_TEST_DATABASE_URL` / `ONTOK_DATABASE_URL` 环境变量，未 echo、未落盘、未入报告 |
| 源库 | **未接触**（无连接串，`.env` 内无）。凡需实跑勘查器的部分标注「因无法访问源库而未验」 |

**环境可用性**：cargo 1.98.1 可用；本体库可达（经变量名引用，端口 30432 代理会重置超额连接，测试期间出现 1 次无关噪声，已隔离判定见 §2.1）。

### 一句话结论

**不通过。** 交付物在**功能上大体成立**（只读保证实测有效、抽取产物与自述数字基本对得上、翻译表 91↔91 精确），但存在 **1 个阻断项**（客户视图 51/51 条关系泄露原始基数记号 `N:1`，守门器完全看不见，且测试对抽取器实际发货的基数语义**零敏感**）和 **1 个阻断项**（E2E 脚本 step2/step6 的校验只打印不判定，AC-103/104/107 的量化判据实际不参与退出码）。此外，**AC-101 的两层安全保证（`after_connect` SET、fail-closed）零测试覆盖，且判定器自测对 fail-closed 假通过** —— 与 REQ-002 B1 同型。

### 9 条自述裁定表

| # | 自述 | 裁定 | 关键证据 |
| --- | --- | --- | --- |
| 1 | 三层只读保证，理由是 sqlx 0.8 不透传 startup 参数 | **部分属实** | 层1 **实测完全无效**（探针 A：`?default_transaction_read_only=on` → 服务端 `default=off` / `transaction_read_only=false`），作者已诚实披露；层2/层3 **实测有效**（探针 B/C：只读采样 6/6、池化 20 次 0 漏网、真实 `CREATE TABLE` 被服务端拒绝；探针 D：无保护时写成功）。**但层2/层3 零测试覆盖，判定器对层3 假通过**（m6/m7/变异A） |
| 2 | 勘查器源码无写操作字面量 | **属实** | 剥注释后 `\b(INSERT\|UPDATE\|DELETE\|TRUNCATE\|DROP\|ALTER\|CREATE)\b` 零命中；全部 SQL 硬编码或 `$1` 绑定；`quote_ident(...)` 是**服务端函数**而非字符串拼接 → **无 SQL 注入面**。（小瑕：`introspect.rs:12` 文档写「只发 SELECT」，实际第 140 行发出 `SET`） |
| 3 | 91 实体==91 表 · 中文 91/91 · 964 属性 · 51 关系 · 悬空 0 | **属实** | 独立复算：1051 列 − 87 个 `id` 主键列 = **964** ✓；91 实体 / 964 属性 / 51 关系 / 悬空 0 / display_name 无空 / 无重名 / 无拉丁残留（除 4 个含 AI·API·IP 的中文名） |
| 4 | 客户视图 91 对象 · 平均 8.7 · 0 零属性 | **属实** | 实测 `out/e2e-view.json`：91 对象 / 793 属性 / **均值 8.714** / 零属性 0 / min 1 · max 26 |
| 5 | 分层匹配后不误杀，S-3 仍被抓 | **部分属实** | 两个测试**有真实判别力**（m2b 改回子串匹配 → `test_view_guard_allows_real_business_key_concepts` 红；m3 白名单恒生效 → 3 个测试红）。**但白名单越界无任何测试覆盖**（§3 M1），S-3 回归测试只测裸词、恰好绕开白名单作用域 |
| 6 | 覆盖率 18%→51% + 认不出仍返回 None | **部分属实** | None 不退化**属实**（m4 塞 `blob_ref_xyz` → 2 个测试红；实测 0 个裸 snake_case 泄露）。**但 51% 是词表单独覆盖率、低于 AC-104 的 60% 目标**，而报告判「✅ 超额达标」；真正达标的端到端数字 **793/1051=75.5%** 报告从未计算（§3 M8） |
| 7 | `--accept-all` 走同一 `set_decision` 与同一套领域校验 | **部分属实** | 同一 `set_decision` **属实**（`cli.rs:346-362`），无独立捷径，clap 互斥正确。**但该函数本身不含任何领域校验**（`db/proposal.rs:183-201` 是裸 `UPDATE`），「同一套领域校验」在该层是空的 —— 措辞高估，但**无功能性绕过**（§3 M10） |
| 8 | 148 全绿 · 零回归 · 零编译警告 | **部分属实** | 148 全绿**我复现了**（基线 148 passed）。**「零回归」不成立**：m5 证明测试对抽取器实际发货的基数语义零敏感。**「零编译警告」未单独统计**（见 §0 未验证项） |
| 9 | 判定器自测 10/10 | **属实，但保证力被高估** | 实跑 `bash verify_selftest.sh` → 通过 10 · 失败 0 · exit 0。**但 10 个里只有 5 个（AC-002）真在执行判据**；AC-101 的 2 条 + AC-105 的 2 条是**源码文本存在性 grep**，我用逐字复制的判据证明：守卫被阉割、守门词改回子串匹配，**仍报 ✓**（§2.3） |

**裁定统计：属实 4 · 部分属实 5 · 不属实 0。**

### 本次实际完成的任务

| 任务 | 状态 | 说明 |
| --- | --- | --- |
| P0-1 只读保证 | ✅ 完成 | 三层逐层实证 + 8 种连接串形态 + 写路径枚举 + 留痕时序 |
| P0-2 新测试判别力 | ✅ 完成 | 9 个变异（含 1 个重跑确认）+ 逐变异 REVERT-CHECK + 4 个定向假通过实验 |
| P0-3 翻译表质量 | ✅ 完成 | 91 条逐条审 + 集合对账 + 守门词/退化/重名/版本链 |
| P0-4 关系推断抽查 | ✅ 完成 | **51 条全量审**（超出 15 条要求）+ 163 个候选列的全量漏判分析 |
| P1 回归检查 | ✅ 完成 | `--accept-all`、Option 报错、测试独立性、`sanitize_comment`（真实库仅 2 列命中） |
| P1 判定器与 E2E | ✅ 完成 | 打印式校验实证、AC 覆盖缺口、三处脆弱性 |
| 源库实跑勘查 | ❌ 未做 | **因无法访问源库而未验**（无连接串）。`introspect()` 的真实端到端行为仅在测试库上以等价探针复现 |
| 编译警告计数 | ❌ 未做 | 未单独统计 `-W` 警告数（见 §未验证项） |
| 词表覆盖率 546/1051 复算 | ❌ 未做 | 探针与变异 harness 争用同一源码目录，为不污染变异结果主动放弃（见 §未验证项） |

---

## 1. 只读保证审查（最高优先级）

勘查器是全平台唯一接触客户数据的代码。逐层结论如下。

### 1.1 三层有效性（实测，非静态阅读）

我在测试库上写了 4 个探针（`tests/zz_probe_readonly.rs`，仅存在于副本），全部只发 `SELECT` 与 `CREATE TEMP TABLE`（临时表，结束即删）：

| 探针 | 构造 | 实测结果 |
| --- | --- | --- |
| **A** | 连接串带 `?default_transaction_read_only=on`，**不加** `after_connect` | `default=off`，`transaction_read_only=false` |
| **B** | 加 `after_connect` 的 `SET` | 6/6 采样 `on/true`；真实 `CREATE TEMP TABLE` **被服务端拒绝**：`cannot execute CREATE TABLE in a read-only transaction` |
| **C** | `after_connect` + `max_connections(3)`，连续借出 20 次 | `non_readonly_borrows = 0/20` |
| **D** | 完全不加任何保护 | `default=off`；`CREATE TEMP TABLE` **成功** |

**结论**：
- **层 1（`force_read_only`）实测完全无效。** 探针 A 独立证实了作者的技术判断：sqlx 0.8 确实不把连接串里的 startup 参数透传给服务端。作者**没有隐瞒**这一点（`introspect.rs:129-137` 与 E2E 报告 §5.1 都写明了），这一点应予肯定。
- **层 2（`after_connect` SET）有效且覆盖池化连接**（探针 B/C）。sqlx 的 `after_connect` 对**每条新建连接**执行，`max_connections(2)` 下不存在漏网连接；探针 C 的 0/20 排除了「连接归还后状态漂移」。
- **层 3（fail-closed）是真正的兜底**。探针 D 证明：若层 2 失效，写操作**真的会成功** —— 所以层 3 不是冗余，它是唯一能在层 2 失效时阻止勘查的机制。

### 1.2 `force_read_only()` 的漏洞

`introspect.rs:93-114`。我先用 3 条既有 Rust 单测的期望**交叉验证**了一个等价的 Python 复刻（3/3 一致），再用它枚举未覆盖的连接串形态：

| 输入 | 输出 | 评价 |
| --- | --- | --- |
| `.../d?default_transaction_read_only=**off**` | **原样返回 `=off`** | 🔴 **不"force"**。函数只判断 `starts_with("default_transaction_read_only=")`（`:107`）就保留，**不覆盖既有值** |
| `.../d?default_transaction_read_only=off&sslmode=require` | 原样保留 `off` | 🔴 同上 |
| `.../d?DEFAULT_TRANSACTION_READ_ONLY=off` | `...=off&default_transaction_read_only=on` | 可接受（PG GUC 名大小写不敏感，后者胜出；且被层 2 覆盖） |
| `.../d?`（空 query） | `?default_transaction_read_only=on` | ✅ |
| `.../d?a=b?c=d`（多个 `?`） | `?a=b?c=d&default_transaction_read_only=on` | 可接受 —— sqlx 解析该 option 时报错，`introspect.rs:121-123` 的 `context` 使其 fail-closed |
| `.../d?default_transaction_read_only`（无值） | 无值参数 + `=on` | 可接受（PG 侧报错 → fail-closed） |
| `.../d?options=-c%20search_path%3Dfoo&sslmode=require` | 完整保留 + 追加 | ✅ URL 编码的 `&` 不在明文，不会被误切 |
| `postgresql://u:p@ho,st/d?sslmode=require`（多主机） | 完整保留 + 追加 | ✅ |

**判定**：无「静默失去只读」或「拼接出可写连接串」的路径 —— 异常形态都以解析失败 fail-closed。**唯一实质问题是 `=off` 不被覆盖**（见 §3 M3）：`force_read_only` 的名字与 AC-101「连接串**强制**追加」的措辞都承诺了 forcing，实现只承诺了「没有就加」。当前因层 1 无效而影响为零，但 sqlx 一旦修复 startup 参数透传，这立刻变成真漏洞。3 条单测覆盖 absent / already-`on` / 不重复，**恰好漏掉唯一违反 force 语义的 `already-off`**。

### 1.3 SQL 注入面

`introspect.rs` 全部 5 条 SQL：3 条 `query_scalar`/`query_as` 用 `$1` 绑定 `TARGET_SCHEMA`；2 条含 `quote_ident(...)` —— 这是**服务端函数调用**（`quote_ident(t.table_schema) || '.' || quote_ident(t.table_name))::regclass::oid`，参数来自 `information_schema` 自身，不是字符串拼接。

全文件唯一的 `format!` 是 `introspect.rs:113` 的 URL 拼接，与 SQL 无关。`db/proposal.rs:196` 的 `format!("UPDATE ... {col} ...")` 中 `col` 来自 `match` 字面量，作者注释也点明了，**无注入面**。

**判定：SQL 注入面 = 无。** 这是本轮质量最好的一处。

### 1.4 TOCTOU 窗口

校验在 `introspect.rs:156-168`，后续查询在 `:170-222`。三点分析：

1. **校验在所有业务查询之前**（`:163` 的 `bail!` 先于 `:170` 起的一切），顺序正确。
2. **校验连接 ≠ 业务查询连接**：校验用 `fetch_one(&pool)`，业务查询也用 `&pool`，`max_connections(2)` 下**可能不是同一条连接**。但由于 `after_connect` 对每条连接都执行（探针 C 实证 0/20 漏网），两条连接状态一致，**不构成实际 TOCTOU**。若有人日后调大 `max_connections`，这个「抽样验证」的语义会变弱 —— 值得记一笔，但不是当前缺陷。
3. **探测 D 证明这是真实防线**：无层 2 时写操作成功，所以层 3 不可省。

**判定：无实际 TOCTOU 窗口。**

### 1.5 产物留痕的可信度

- **时序**：`transaction_read_only` 在 `:156-161` 读取，**早于**所有业务查询（`:170` 起）。所以它证明的是「**开始勘查前**服务端已是只读」，而不是「勘查后仍是只读」。对只读保证这个命题，前者才是需要的 —— 留痕时序**正确且更强**。
- **语义**：字段名叫 `transaction_read_only`，实际读的是 `current_setting('default_transaction_read_only')`（`:157`）。**两个 GUC 不是一回事**（前者是当前事务的实际只读态，后者是新事务的默认值）。字段名与所测量的量不符。产物 `out/e2e-schema.json` 的 `"transaction_read_only": "on"` 因此是「默认值已设为 on」的留痕，不是「当前事务只读」的留痕。
  - 实际风险为零：探针 B/C 显示两者在勘查期间始终一致（`on/true`）。
  - 但**留痕的字段名会误导外部审计者** —— 一个只读审计者会认为该字段证明了当前事务只读。建议改名为 `default_transaction_read_only`，或额外记录真正的 `transaction_read_only`。
- **E2E 只检查这个字段的字面值**（`e2e-req003.sh:38-39`），`[ "$RO_FLAG" = "on" ] && ok || bad` —— 这处**参与判定**（正确）。
- 另注意 `ontok-introspect.rs:46` 与 `propose.rs:180-183` 都把这个值写进 `extracted_by.source`，溯源链完整。

---

## 2. 新测试的判别力

### 2.1 变异结果表

方法学：每变异**独立 `CARGO_TARGET_DIR`**（遵 REQ-002 复审教训），每变异做 **REVERT-CHECK**（与原始 worktree 逐字节 `diff`），**没有还原验证的结果一律不采信**。

> ⚠️ 过程中的一次自纠：首版 harness 的备份路径有 bug（`_backup/src/domain/view.rs` 父目录不存在），导致 m1 的变异残留、m2–m5 全部编译失败。**这批结果已作废**，从原始 worktree 逐字节还原后用修正版 harness 重跑。下表全部来自重跑结果，每行均有 `REVERT-CHECK: OK`。

| ID | 注入的变异 | 测试结果 | 结论 |
| --- | --- | --- | --- |
| **m1** | 从 `BUSINESS_WORDS` 删除 5 个新增词条（`id`/`created_at`/`updated_at`/`created_by`/`is_deleted`） | 🔴 **RED** `test_humanize_real_world_audit_columns` | ✅ **有判别力**。词表增补的测试不是假断言 |
| **m2b** | `english_term_hit` 的词元匹配改回**子串匹配**（`text.to_lowercase().contains(t)`） | 🔴 **RED** `test_view_guard_allows_real_business_key_concepts` | ✅ **有判别力**。AC-105 的核心测试守得住 |
| **m3** | 业务前缀白名单**恒生效**（`if i > 0 && ...` → `if i >= 0`） | 🔴 **RED** ×3：`test_view_blacklist_scanner_catches_a_planted_term`、`test_view_blacklist_scanner_is_case_insensitive`、`test_view_guard_catches_key_in_serialized_view` | ✅ **有判别力**（详见下方注） |
| **m4** | 把 `blob_ref_xyz` 塞进 `BUSINESS_WORDS` | 🔴 **RED** ×2：`test_humanize_column_unknown_returns_none_not_raw`、`test_humanize_still_refuses_unknown_columns` | ✅ **S-2 防线有牙齿**。词表增补没有把 S-2 一起放宽 |
| **m5** | `REL_SUFFIXES` 的 `("_id","N:1")` → `("_id","1:N")`，**51 条关系基数语义全部反转** | 🟢 **GREEN 157/157** | 🔴 **零判别力**。见 §3 B1 |
| **m6** | **删除 `after_connect` 的整个 `SET default_transaction_read_only = on`** | 🟢 **GREEN 148/148** | 🔴 **零判别力**。唯一 1 个失败是 `test_apply::test_chained_apply_keeps_accumulating` |
| **m6b** | 同 m6，**重跑确认** | 🟢 **GREEN 148/148** | ✅ 确认 m6 首跑那 1 个失败是**环境噪声** |
| **m7** | fail-closed 的 `bail!` 改成 `eprintln!`（**保留 `if read_only != "on"` 条件**，守卫逻辑失效） | 🟢 **GREEN 148/148** | 🔴 **零判别力**。见 §2.3 变异 A |
| **m8** | **删除 `let safe_url = force_read_only(url)?;` 调用**（helper 变死代码，连接串原样使用） | 🟢 **GREEN 148/148** | 🔴 **零判别力**。3 条单测只测函数，不测调用点 |

**关于 m6 首跑那 1 个失败的归因（严格区分环境噪声 vs 真实缺陷）**：
- 该测试是 `test_apply.rs::test_chained_apply_keeps_accumulating`，操作**本体库**，与 `introspect.rs` 无任何因果路径（`after_connect` 只挂在勘查器的 source 库连接池上）。
- 在**未变异树**上连续重跑 3 次：**3/3 通过**。
- m6b 重跑变异体：**148/148 全绿**，该失败不复现。
- 结论：**环境噪声**（端口 30432 代理重置超额连接，需求 §6 S6 已登记），非产品缺陷。我没有用它掩盖任何真实失败 —— 恰恰相反，它掩盖的正是「删掉唯一有效的只读层」这个真实覆盖空洞。

**关于 m3 为何没让 `test_view_guard_still_catches_leaked_identifiers` 变红**（这是理解判别力边界的关键）：
该测试只测**裸词**（`Key`/`KEY`/`CARDINALITY`/`primary_key`/`PK`/`pk`）。裸词在 `english_term_hit` 的**第 1 步整体匹配**（`view.rs:462-467`，`whole == *t`）就被拦下，根本走不到第 2 步的词元循环，所以白名单怎么改都不影响它。**白名单真正的作用域（多词元文本）只被 3 个 JSON 形态的测试覆盖** —— 这正是 §3 M1 那个洞能存在的原因。

### 2.2 无判别力或覆盖不足的测试

| 缺口 | 触发条件 | 证据 |
| --- | --- | --- |
| 抽取器发货的**基数语义**零覆盖 | 改 `REL_SUFFIXES` 的 cardinality | m5：51 条关系语义全反，157/157 绿 |
| `introspect()` 的**只读运行时行为**零覆盖 | 删 `after_connect` / 阉割 fail-closed | m6/m6b/m7：148/148 绿 |
| `force_read_only` 的**调用点**零覆盖 | 删调用，helper 变死代码 | m8：148/148 绿 |
| 客户视图**真实产物**零断言 | —— | 无任何测试读 `out/e2e-view.json`；B1 的 `N:1` 泄露因此无人看见 |
| 守门词白名单**越界方向**零覆盖 | 白名单前缀 + 非 `key` 建模术语 | §3 M1：10 个形态实测无命中 |
| 守门器**只报第一个命中** | 多个英文违规同时存在 | `view.rs:460` 返回 `Option<&'static str>`，`scan_modeling_terms:508-512` 只推 1 个英文词进 `hits`。不影响退出码（任一命中即拦），但报告只说 1 个违规词 |
| `props_dropped` 是**死统计** | —— | `propose.rs:150` 声明，全仓库无 `+= 1`。见 §3 M2 |

### 2.3 判定器自测对安全保证的假通过（P0 关键）

`verify_selftest.sh` 里 AC-101 的 2 条 + AC-105 的 2 条是**源码文本存在性 grep**，不是行为判据。我把它的 5 条判据**逐字复制**出来，喂给变异体：

```
--- 基线（未变异） ---            --- 变异A：bail→eprintln ---      --- 变异C：守门词改回子串匹配 ---
  ✓ [1] 无写操作字面量               ✓ [1] 无写操作字面量              ✓ [1] 无写操作字面量
  ✓ [2] 勘查器强制只读               ✓ [2] 勘查器强制只读              ✓ [2] 勘查器强制只读
  ✓ [3] 勘查器 fail-closed           ✓ [3] 勘查器 fail-closed   ←假通过 ✓ [3] 勘查器 fail-closed
  ✓ [4] 守门词已分层                 ✓ [4] 守门词已分层          ←假通过 ✓ [4] 守门词已分层
  ✓ [5] 存在业务前缀白名单           ✓ [5] 存在业务前缀白名单    ←假通过 ✓ [5] 存在业务前缀白名单
```

（变异 B = 整段删除 fail-closed 校验 → 判据 3 正确报 ✗。即：**能抓「整段删除」，抓不住「阉割」。**）

- **变异 A**（守卫逻辑失效、条件字面量保留）5/5 全 ✓ → `verify_selftest.sh` 会照样报「通过 10 · 失败 0」。这正是 REQ-002 复审指出的「删掉守卫逻辑但把字面量留在代码里」绕过。
- **变异 C**（守门词改回子串匹配、两个常量名保留）5/5 全 ✓ → 判据声称「守门词已分层 ✓ 存在业务前缀白名单 ✓」，而行为已完全退化为 REQ-002 修掉的那个版本。**（注：m2b 证明 `cargo test` 仍能抓到这个回归 —— 抓不到的是判定器自测。）**
- 判据 1（写操作字面量）**是有效的**：`verify_selftest.sh:103-104` 与 `e2e-req003.sh:50-51` 配对正确且真的会执行 grep。
- 判据 2/3/4/5 全部满足于「字符串出现在文件里」，无论该字符串是否被使用。
- **AC 覆盖缺口**（作者已在 E2E 报告 §5.3/§6 **主动披露**，此处独立确认）：10 个用例中，AC-002 有 5 个真判据；**AC-102 / AC-103 / AC-104 / AC-106 / AC-107 共 5 条 AC 零自测**。AC-101 与 AC-105 名义上有 5 个用例，但其中 4 个无行为判别力。

---

## 3. 发现清单（按严重度）

### 🔴 B1 · 阻断项 · 客户视图 51/51 条关系泄露原始基数记号 `N:1`，守门器完全看不见

**触发条件**：任何走 `ontok ontology show` 的客户视图（生产路径，无特殊配置）。

**链条（三处独立失守）**：

1. **生产者发出未被识别的记号**。`src/extract/propose.rs:161` REQ-003 新增的 `REL_SUFFIXES` 对**每一个** `*_id` 列发出 `cardinality: "N:1"`。
2. **消费者不认这个记号**。`src/domain/view.rs:269-276` 的 `cardinality_cn` 只映射 `"1:1"`→唯一对应、`"1:N"`→一对多、`"N:M"`→多对多 —— **没有 `N:1` 分支**，落到 `_ => c.to_string()`，把 `"N:1"` **原样透给客户**。
3. **守门器看不见它**。`MODELING_TERMS_CJK`（`view.rs:426`）只含「外键/基数/主键/证据」四个**中文词**，不含 `N:1` 这个**记号**。我的探针对整个真实产物（137,663 字节）跑 `scan_modeling_terms` → **返回空数组**。所以 `ontok ontology show` 正常 exit 0，E2E step6 判「✓ 守门器未拦截 → 无建模术语泄露」。

**实测证据**：
- `out/e2e-view.json`：`"how_many": "N:1"` 出现 **51 次**；51/51 条关系全部如此。
- 样例：`{"from": "活动关键词", "to": "活动", "how_many": "N:1", "meaning": "归属于"}`
- 探针 `PROBE_VIEW hits=[] len=137663`

**违反的验收**：
- **AC-005** —— `view.rs:3-4` 文档明写「客户看到的必须是业务语言，不能出现任何建模术语（主键 / **基数** / 外键 / ordinal / key …）」。`N:1` 就是基数记号。
- **AC-106** —— 「客户视图**零建模术语**（复用 REQ-002 的运行时守门）」。运行时守门对本形态**结构性无效**。

**影响**：客户视图 100% 的关系条目都带原始建模记号。这直接打击需求的核心赌注 —— E2E 报告 §5.2 称 `License Key` 误杀会导致「`cli.rs` 的运行时守门 `exit 2` 关闭**整个**客户视图」，作者为 AC-105 付出了很大代价；而 `N:1` 却是从**守门器的盲区**直接走进客户视野的，代价为零。

**为什么 148 个测试看不见（m5 实证）**：
把 51 条关系的基数从 `N:1` 全部反转为 `1:N`（**关系基数语义完全颠倒**），**157/157 全绿**。根因是覆盖不对齐：
- 全仓库 `grep -rn 'N:1' src/ tests/` 只命中 `propose.rs:161-163` 三处**生产者**。
- REQ-002 的消费者侧测试 `test_view_relations_use_human_words` 只断言 `"1:N" → "一对多"`，**从未测过 `N:1`**。
- 即：生产者发 4 种基数，消费者只认 3 种，**两端的差集没有任何测试**。

**修法方向**（不属我的产出，仅给方向）：`cardinality_cn` 补 `N:1` 分支；并在 `REL_SUFFIXES` 的每个 cardinality 字面量上加一条「该值必须能被 `cardinality_cn` 翻译」的测试，把两端的契约钉死（这才是根治 —— 否则下次加一种记号又会漏）。

### 🔴 B2 · 阻断项 · E2E 脚本 step2/step6 的校验只打印，不参与退出码

**触发条件**：`bash BA/e2e-req003.sh` 的 step 2（9 项校验）与 step 6（avg≥3 / 零属性对象）。

**机制**：这两段是 python heredoc（`e2e-req003.sh:66-88` 与 `:127-142`），只 `print('✓'/'✗')`。**python 子进程无法修改 shell 的 `FAIL` 变量**；脚本 `set -uo pipefail`（**无 `-e`**），最终 `exit $FAIL`（`:162`）。`FAIL` 只由 `bad`/`chk` 设置，而这两段没有任何 `bad` 调用。

**实测（我构造了明显有病的数据，喂给脚本原文的校验块）**：

```
=== step2 校验块（project 错 / 实体 1≠91 / display_name 全空 / 悬空 7 / 无版本 / 关系两端缺表）===
  ✗ 提案 project 字段 == 实际项目（WRONG_PROJECT）
  ✗ 实体 1 个 == 源库 91 张表
  ✗ 全部实体有中文 display_name（0/1）
  ✗ 属性 0 个（嵌套在实体 payload 内）
  ✗ 关系 1 条，悬空 7 条
  ✗ extracted_by 带版本：a v
  ✗ extracted_by 留痕只读勘查：
  ✗ 关系两端均在实体集合内
  ✓ 实体 key 唯一
>>> python 退出码 = 0          ← 8 项打 ✗，脚本仍判通过

=== step6 校验块（avg=0.0 / 零属性对象=2）===
  ✗ 平均属性数/对象：0.0（REQ-002 真实数据基线 2.2，目标 ≥3）
  ✗ 零属性对象：2 个（REQ-002 时 14 个）
>>> python 退出码 = 0
```

**因此以下判据实际不参与判定**：
| 判据 | 所属 AC | 现状 |
| --- | --- | --- |
| 全部实体有中文 `display_name` | AC-103 / AC-104（实体存活率 100%） | 不参与 |
| 属性数 == `total_properties` | AC-103 | 不参与 |
| 关系两端均在实体集合内 | AC-103 / AC-007 | 不参与 |
| 实体 key 唯一 | AC-103 | 不参与 |
| `extracted_by` 带版本 + 留痕只读 | AC-107 | 不参与 |
| **平均属性数 ≥ 3** | **AC-106** | 不参与 |
| **零属性对象 == 0** | AC-104 | 不参与 |

**部分缓解（如实说明）**：悬空关系由 `ontok-propose.rs:88-90` 的 `bail!` 间接拦住 —— `e2e-req003.sh:65` 的 `|| { bad "抽取失败"; exit 2; }` **确实**参与判定。所以「悬空 > 0」会失败，但那是 bin 的行为，不是脚本的校验。

**加重情节**：
- E2E 报告 §0 把「2 抽取本体 ✓ **9 项校验全过**」和「6 客户视图 ✓ …平均 8.7 属性 · 0 零属性」列为通过项，读者会以为它们是判定项。
- 同一脚本内两种写法不一致：step 4（`:104-108`）用 `if <pipeline>; then ok; else bad; fi`（**正确**，`pipefail` 下会捕获失败），step 2/6 则是纯打印。作者在 `:101-103` 还专门写了注释说明「走同一条 `set_decision` 路径」——对领域逻辑很谨慎，对自己的判定脚本却重犯了这个坑。
- 讽刺的是：作者在 E2E 报告 §5.2 把「E2E 太慢」的根因分析得很准，却在同一个文件里让 9 项校验退化成日志输出。

### 🟠 M1 · 守门词业务前缀白名单越界：能屏蔽掉所有英文建模术语，不只是 `key`

`view.rs:483`：
```rust
if i > 0 && BUSINESS_ALLOWED_PREFIXES.contains(&toks[i - 1].as_str()) { continue; }
```
这行对 `MODELING_TERMS` 里**每一个**词都生效，而白名单是为「`license_key` 里的 `key` 是业务概念」设计的。

**实测无命中（10 个形态）**：
```
config_primary_key   → 无命中    config_cardinality → 无命中    config_ordinal   → 无命中
cache_extracted_by   → 无命中    config_evidence    → 无命中    config_disjoint  → 无命中
api_proposed_by      → 无命中    api_pk             → 无命中    sort_ordinal     → 无命中
record_evidence      → 无命中    config_key_extra   → 无命中
```
`primary_key` / `extracted_by` / `proposed_by` / `pk` / `ordinal` / `evidence` / `disjoint` / `cardinality` 是**纯建模术语、零业务读法**，被一并放过。

**为何测试没抓到**：S-3 回归测试 `test_view_guard_still_catches_leaked_identifiers` 只测**裸词**，裸词走 `english_term_hit` 第 1 步的整体匹配（`view.rs:462-467`）先命中，永远到不了白名单所在的第 2 步词元循环（m3 分析已证）。**白名单真正的作用域零覆盖。**

**可达性评估（诚实）**：在当前真实产物里我没有找到实际被放过的例子（`e2e-view.json` 的 145 个含拉丁属性名里没有 `xxx_primary_key` 这类形态）。**这是一个尚未被触发的防线洞，不是已发生的泄露。** 但它与 B1 同向：都属「守门器在真实数据形态上的盲区」，而本平台的客户视图守卫是 AC-005/AC-106 的最后一道闸。属性名直接来自源库列注释（见 M12），源库注释是完全不可控的外部输入。

**修法方向**：白名单豁免只应作用于 `key`（必要时含 `pk`），不应作用于词组类与 `ordinal`/`evidence`/`disjoint`/`cardinality`；并补一条「白名单前缀 + 其它建模术语仍被抓」的测试。

### 🟠 M2 · `props_dropped` 是死统计，恒为 0

`propose.rs:150` 声明 `pub props_dropped: usize`，注释写「认不出、会被 `project()` 丢弃的属性数」。全仓库 `grep -rn props_dropped src/` **只有这一处声明，没有任何 `+= 1`**。

`out/e2e-stats.json` 的 `"props_dropped": 0` 与 `"props_via_wordlist": 345` 并列出现在 E2E 报告的证据表里，但它是**结构上不可能非零**的数字 —— 就像 REQ-002 的 B1，是「装饰性防线」。

真正该记的位置在 `view.rs:314-333` 的 `filter_map` 丢弃处，但那里根本没有回传到 `ExtractionStats` 的通道（`project()` 是纯函数，不返回统计）。

（注：实际被丢弃的属性数可从别处推出 —— 964 属性 → 793 进入视图，差 171 被 `humanize_column` 认不出或 `usable_as_human_name` 判退化而丢弃。这个真实数字没有被记录在任何地方。）

### 🟠 M3 · `force_read_only` 不 force：`=off` 原样保留，且单测恰好漏掉该形态

详见 §1.2。`introspect.rs:107` 只判断存在性、不覆盖既有值；`?default_transaction_read_only=off` → 原样返回 `=off` 且函数返回 `Ok`。

3 条单测（`:283-302`）覆盖 absent / already-`on` / 不重复 —— **唯一违反 force 语义的 `already-off` 恰好没测**。

当前影响为零（层 1 实测无效，§1.1 探针 A），属**潜伏契约违背**：函数名与 AC-101「连接串**强制**追加 `default_transaction_read_only=on`」都承诺 forcing。若 sqlx 修复 startup 参数透传，它会立刻变成「调用方传入 `=off` 即可关掉第 1 层」的真漏洞。

**还该测而没测的连接串形态**（我已全部枚举并实测，见 §1.2 表）：多 `?`、`=off`、大写 GUC、无值参数、URL 编码 `&`、多主机。前 5 种当前都能安全 fail-closed，但**无任何测试锁住这个行为**。

### 🟡 M4 · 关系推断的 `*_id` 约定只命中 31%，漏的正是注释已证实的真关系

我没有只抽查 15 条，而是**全量审了 51 条**，并对全部 **163 个**符合 `*_id`/`*_ids`/`*_no`/`*_code` 约定的列做了漏判分析。

| 指标 | 数值 |
| --- | --- |
| 符合命名约定的列 | **163**（`_id` 145 · `_code` 17 · `_no` 1 · `_ids` **0**） |
| 推断出关系 | **51（31%）** |
| 因「目标表不存在」跳过 | **112** |

**根因与 `singular_to_table` 的设计意图相反**。该函数（`propose.rs:362-368`）精心处理了三条复数还原规则，但本 schema 的 `*_ids` 列数是 **0** → **这三条规则连同 `REL_SUFFIXES` 的 `("_ids","1:N","has")` 分支对本 schema 全是死代码**。真正的失败模式是**词干比表名短**（业务名词 ≠ 表名），复数还原解决不了。

**实证漏掉的真关系（列注释直接写明目标）**：

| 列 | 注释 | 应指向 | 漏判原因 |
| --- | --- | --- | --- |
| `llm_provider.model_id` | 「关联模型ID(对应llm_model.model_id)」 | `llm_model` | 词干 `model` ≠ 表名 `llm_model` |
| `llm_model_peak_config.model_id` / `llm_model_sort.model_id` / `llm_provider_back_20260701.model_id` / `llm_provider_groups.model_id` / `llm_routing_rules.model_id` | 「模型ID」「关联模型ID」 | `llm_model` | 同上（共 **6 条**指向最核心的大模型实体，全部漏） |
| `admin_user_roles.role_id` | 「角色ID」 | `admin_roles`（表存在） | 词干 `role` ≠ `admin_roles` |
| `balance_transfer.binding_id` / `frozen_assets.binding_id` | 「关联的绑定记录ID」 | `customer_account_binding`（表存在） | 词干 `binding` ≠ `customer_account_binding` |
| `main_customer_id` × **7 张表** | 「主账号ID」「主账号ID（冻结操作者）」 | 客户/主账号 | 词干 `main_customer` 无匹配表，**7 条全漏** |
| `subscription_id` × 5 | 「订阅ID」 | `customer_subscriptions` | 词干 `subscription` ≠ 表名 |
| `service_id` × 3 | 「服务标识」 | `service_apps` / `service_config` | 词干 `service` ≠ 表名 |
| `llm_provider_group_items.provider_id` / `.group_id` | 「Provider ID」「Provider组ID」 | `llm_provider` / `llm_provider_groups` | 词干 `provider`/`group` ≠ 表名 |
| `share_usage.shared_user_id` | 「被分享人ID」 | `customers` | 词干 `shared_user` 无匹配 |

**一个正面例子**：`customer_interests.pre_order_id`（「第三方订单id」）目标表不存在 → **正确跳过**，未误判成 `order`。这证明「目标表必须存在」这道闸有效 —— 但正如需求 §5.1 所说，**目标表存在不等于真是关系**（见 M5）。

**一个结构性后果**：`family_high_risk_records`（同设备高风险记录）全表只有 `main_customer_id`、没有 `customer_id` → **0 条关系**，在客户视图里与其它 90 个对象**完全孤立**，无任何上下文。

### 🟡 M5 · 41/51 关系指向 `customers`，把「客户/账号/主账号/子账号/用户」压平成一个概念

**分布实测**：51 条关系只有 **6 个不同目标表** —— `customers` **37**、`admin_users` 5、`licenses` 4、`order` 3、`activity` 1、`share_usage` 1。**全部 51 条都是 `N:1` + `belongs_to`**（`REL_SUFFIXES` 只有 `_id` 命中，见 M4）。

**源库注释明确区分了多种主体，但关系层把它们全压成「客户」**：

| 列 | 注释原文 | 推断目标 | 问题 |
| --- | --- | --- | --- |
| `frozen_assets.customer_id` | 「**子账号ID**」 | `customers` | 🔴 注释明说是子账号，同表另有 `main_customer_id`（「主账号ID（冻结操作者）」）。指向 `customers` 很可能**错** |
| `balance_operation_log.customer_id` | 「账号ID（余额所属账号）」 | `customers` | 存疑：「账号」≠「客户」 |
| `customer_interests.customer_id` | 「**用户id**」 | `customers` | 存疑：终端用户 vs 客户账号 |
| `operate_log.customer_id` | 「用户 ID」 | `customers` | 🟠 **审计日志表** —— 正是 briefing 点名的「日志 actor 字段」风险 |
| `marketing_channel_log.customer_id` | 「用户id」 | `customers` | 🟠 变更日志表，同上 |
| `dim_customer_binding.customer_id` | 「账号ID（统计维度账号）」 | `customers` | 🟠 **维表**：维度表按定义聚合多个账号，「归属于 客户」对客户是误导 |
| `rpt_dim_customer.customer_id` | 「客户ID」 | `customers` | 🟠 **报表表**（`rpt_` 前缀）：把报表表暴露成业务对象并挂关系，是噪声不是业务语义 |
| `app_billing_transactions.customer_id` | 「**被代扣Open用户ID**」 | `customers` | 🟠 注释说的是「Open 用户」，与 `customers` 未必同一集合 |
| `internal_staff.customer_id` | 无注释 | `customers` | 🟡 内部员工表挂 customer_id，多租户下可能合理，但对客户是怪异语义 |
| `usage_reconciliation_log.customer_id` | 无注释 | `customers` | 🟡 对账日志，同审计日志风险 |

**`COLUMN_LABELS` 已有的区分没被用上**：`view.rs:13-25` 里 `main_customer_id`→「主客户」、`customer_id`/`cust_id`→「所属客户」是分开的，但**关系推断层没有对应的区分** —— 7 处 `main_customer_id` 全被跳过（M4），而 `customer_id` 一律指向 `customers`。

**对客户的影响**：客户视图里 41/51 条关系显示为「XX **归属于 客户**」，配合 B1 的原始记号 `N:1`，会把「子账号冻结资产归属于客户」「操作日志归属于客户」这类错误结构直接呈现给业务方。

**关于基数 N:1 本身是否合理**（briefing 明确问了）：对**单列外键**而言 `N:1 belongs_to` 在「多个子行指向同一个父行」这个读法下是**正确的**（多条明细行 → 一个客户）。所以基数**取值本身不算错**，错的是 (a) 方向被原样透成 `N:1` 记号（B1），(b) `belongs_to` 的中文是「归属于」，对「订单 归属于 客户」自然，但「对账日志 归属于 客户」就误导，(c) 目标表选错（M5）。

**关于 `singular_to_table` 够不够用**（briefing 问了两个具体例子）：
- `activity_keys` → 目标 `activity`：`infer_link` 对列 `activity_keys` 先试 `_ids` 后缀 → 词干 `activity_key` → 候选 `activity_keys`（= 列名，被 `cand != c.name` 排除）、`activity_key`（无此表）、`singular_to_table("activity_key")` → 剥 `s` → `activity_key`（无此表）→ **无关系**。**注意：这里推断的是 `activity_keys.activity_id → activity`（正确），不是从表名反推。** 表名 `activity_keys` 复数还原没被用到。
- `admin_user_roles` → 目标 `admin_users`：`admin_user_roles.admin_user_id` → 词干 `admin_user` → 候选 `admin_users` ✓ → 关系正确。**表名 `admin_user_roles` 的复数还原同样没被用到**（真正命中的是列名 `admin_user_id` → `admin_users`）。
- 结论：两个例子都**推断正确**，但都**不是靠 `singular_to_table` 答对的** —— 是靠「列名词干 + 简单加 `s`」这条候选顺序。该函数在本 schema 上一次都没被真正需要（`*_ids` 列数为 0）。

### 🟡 M6 · 翻译表错译：`customer_interests` → 「客户意向」

**证据**（同表列注释）：
```
id           '主键'
customer_id  '用户id'
interest_id  '权益id'          ← 权益，不是「意向」
purchased_at '购买时间'        ← 已购买
expired_at   '到期时间'        ← 有有效期
status       '状态 1:已激活, 0:待激活'
order_id     '系统订单序列号id'
```
这是**客户已购权益（订阅/会员权益）**表，不是「客户意向 / 销售线索」。正确译名应为「客户权益」或「客户已购权益」。

**佐证**：同一 schema 里 `interest_configs` → 「利息配置」（金融利息）。同一个英文词在两处被译成两种完全不同的中文含义（意向 vs 利息），**至少一处是错的** —— 而 `customer_interests` 这一处可由同表列注释确定为错。

**影响**：「客户意向」在中文商业语境里指销售线索/购买意愿。一个显示「客户意向」的对象，属性是「权益ID/购买时间/到期时间/是否激活」，会严重误导读图的人 —— 这恰好是 AC-013（3 人判读）要抓的东西，而 AC-013 尚未执行。

### 🟡 M6b · 存疑翻译：`family_high_risk_records` → 「同设备高风险记录」

全表 11 列（`id` / `main_customer_id` / `negative_debt_rmb` / `threshold_rmb` / `status` / `created_by` / `created_at` / `cleared_at` / `cleared_by` / `remark` / `whitelist_until`）—— **没有任何 device 相关列**，也没有 customer_id。「同设备」这个译法在 schema 里**找不到依据**。

从列看，这更像「同一客户/账号下的高风险（负余额/风控）记录」，`family` 可能指「同族/同组账号」。**标注为「存疑」**：要确认需看真实业务语义（我无源库访问权）。作者应复核这条。

### 🟢 M7 · 翻译表其余部分质量良好（正面结论，独立验证）

以下是我对全部 91 条的机械核对结果，**均为通过**：

| 检查项 | 结果 |
| --- | --- |
| 条目数 vs 源库表数 | **91 ↔ 91 精确一一对应** |
| 源库有、翻译表缺（漏译） | **0** |
| 翻译表有、源库无（幻觉/过期条目） | **0** |
| 两个表译成同一中文名（重名） | **0**（`Counter` 全为 1） |
| 中文名为空 | **0** |
| 中文名 == 表名（退化值） | **0**（`names.rs::load` 校验 + 实测双保险） |
| 中文名含守门词（外键/基数/主键/证据） | **0** |
| 中文名会被 `usable_as_human_name` 判退化（词元撞实体 key） | **0**（中文名分词后不撞 ASCII 表名 key） |
| 中文名含拉丁字母 | 4 条：`AI 检索流水线`、`API 调用日志`、`IP 黑名单`、`IP 白名单` —— 守门器不误杀，但半中半英，AC-013 判读时可能被质疑 |
| 源库拼写错误处理 | ✅ **正确**：`seetings`→系统设置、`intance_task_plans`→实例任务计划。翻译表以**源库表名为 key 精确匹配**，不做拼写纠正，因此源库侧的 typo 不会造成漏译（briefing 特别问的两个例子都正确） |

**抽查的合理翻译**（确认无错译）：`order`→订单、`licenses`→授权凭证、`licenses_archive`→历史授权凭证、`license_quota`→授权额度配置、`deductions`→扣费流水、`refund_operations`→退款操作、`gift_codes`→兑换码、`usage_quotas`→用量额度、`alembic_version`→数据库迁移版本、`proxy_operation_records`→代理操作记录。

### 🟡 M8 · AC-104 的判定口径被替换，报告里的数字自相矛盾

**AC-104 原文**（`demand.md:97-108`）：
- 目标表：属性投影覆盖率 **18% → ≥ 60%**
- 判定：「拿真实源库的 **1068** 列跑新投影，统计覆盖率，`< 60%` 判 `✗`」

**实际发生的三件事**：

1. **分母换了**：实际勘查产出 **1051** 列（`out/e2e-schema.json` 的 `column_count`）。REQ-002 评估报告与 `demand.md` 都写 **1068**。**17 列的差异在产物、代码注释、E2E 报告里都无任何解释**。（`propose.rs:12` 自己也写 1051，与 `demand.md:37` 的 1068 不一致。）

2. **报告的 51% 低于 AC-104 的 60% 目标**。E2E 报告 §3 写「词表增补后覆盖 — **51%（546/1051）**」，而 §1 判 AC-104「✅ **超额达标**」。依据是「平均 8.7 属性/对象」—— 那是**另一个量纲的指标**（属性/对象 vs 列可翻译率），**AC-104 原文里没有它**。读者看到的是「51% < 60%」配「✅ 超额达标」。

3. **真正能证明 AC-104 的数字从未被计算**：端到端投影覆盖率 = 进入客户视图的属性 793 / 源库列 1051 = **75.5%**。这个数在报告里找不到。

**判定**：按 AC-104「列注释优先，词表兜底」的合理解读，**AC-104 实际是达标的（75.5% ≥ 60%）**，投影改进确实解决了需求 §5.2 的核心风险。**问题在报告口径而非产品** —— 缺一个把 51%（词表单独）与 75.5%（端到端）讲清楚的说明，导致「超额达标」这个结论看起来是自相矛盾的。AC-104 的**原始判定式（1068 列 / <60% 判 ✗）从未被任何判据执行过**。

### 🟡 M9 · `out/proposal.json` 与 `out/extract-stats.json` 是陈旧产物，形状与当前代码不符

| 文件 | 时间 | 顶层形状 | 与当前代码是否一致 |
| --- | --- | --- | --- |
| `out/proposal.json` | 08:41 | `{base_project, base_version, entity_types[91], properties[964], link_types[51], extracted_by}` | ❌ **不一致**。当前 `Proposal`（`propose.rs:119-125`）是 `{project, base_version, round, extracted_by, items}`；且旧文件**没有 `round` 字段** |
| `out/extract-stats.json` | 08:41 | 缺 `total_properties` 字段 | ❌ **不一致**。`propose.rs:291` 无条件设置该字段 |
| `out/e2e-proposal.json` | 09:39 | `{project, base_version, round, extracted_by, items}` | ✅ 一致 |
| `out/e2e-stats.json` | 09:39 | 含 `total_properties: 964` | ✅ 一致 |

即：`out/` 下**不带 `e2e-` 前缀的两个产物是上一版代码的遗留**（旧版提案格式正是 E2E 报告 §4 问题 #3 修掉的那个 `entity_types`/`properties`/`link_types` 三数组形状）。需求 §7 交付物第 4 项写的是「提案产物 `out/proposal-*.json`」，与实际交付的 `e2e-proposal.json` / 陈旧的 `proposal.json` 都不对应。

**影响**：审阅者若打开 `out/proposal.json`（briefing 里点名的「抽取提案」），会拿到一个**当前代码根本产不出、且 `ontok propose import` 会拒收**（E2E 报告 §4 问题 #3：「报提案缺 project」）的文件。溯源链上这是实质性的证据污染。

### 🟡 M10 · `--accept-all`：无功能性绕过，但自述 #7 的措辞高估了保护力

**属实的部分**：
- `cli.rs:346-362` 的 `accept_all` 分支与逐条路径调的是**同一个** `proposals.set_decision(it.id, t, Decision::Accepted)`，**没有独立的捷径或简化路径**。
- clap 的 `conflicts_with_all = ["item","track","accept","reject"]`（`cli.rs:126`）正确挡住了混用 —— 这也是 `item`/`track` 改成 `Option` 的原因，作者的推理正确。
- **下游保护仍在**：E2E step5 的 `proposal apply`（`e2e-req003.sh:115`）用 `|| bad "apply 失败"`，**参与判定**，且实测成功；AC-007 的关系两端校验在 apply 侧生效。所以「批量接受会绕过领域校验」这个担忧**不成立**。

**高估的部分**：
- `set_decision`（`db/proposal.rs:183-201`）本身就是一条裸 `UPDATE proposal_items SET {col} = $2 WHERE id = $1`，**不含任何领域校验**。所以「同一套领域校验」在被引用的这个函数层面**是空的**。真正提供保护的是 apply 阶段的校验，不是 decide 阶段。
- E2E 报告 §5.2「走同一条 `set_decision` 路径与同一套领域校验（AC-007 照常生效）」把「同一条路径」和「同一套校验」并列陈述，会让读者以为 decide 环节本身有校验兜底。

**一处被放大但非本轮新引入的治理面**：`set_decision` **不记录 actor**。逐条敲 284 次和一条 `--accept-all` 翻转 284 条，**审计留痕都是零**。这放大的是 REQ-002 已登记的 G-5（同轨 `Accepted` 可抹掉显式 `Rejected` 且无审计，需求 §6 记为「待产品拍板，不擅自决定」）—— **不是 REQ-003 引入的新缺陷**，但 `--accept-all` 让它的影响面从「一次一条」变成「一次 284 条」。

**`item`/`track` 改 `Option` 后的报错可读性（briefing 明确问了）——无回归**：
- 缺 `--item`：`Error: 请给 --item <序号>，或改用 --accept-all 批量接受`（`cli.rs:355` 的 `anyhow::Context`）
- 缺 `--track`：`Error: 请给 --track <biz|model>，或改用 --accept-all 批量接受`
- 两者都缺且无 `--accept/--reject`：先命中 `cli.rs:353` 的 `accept == reject` → `Error: --accept 与 --reject 必须且只能给一个`（顺序合理，先报最根本的）
- 提示语直接给出替代方案，**可读性良好，比改 `Option` 前更好**。

### 🟢 M11 · `sanitize_comment`：真实库上只影响 2 列，且行为符合设计意图

`propose.rs:320-329` 按 `|` 和换行切分取首段。**全库实测**：
- 含 `|` 的列注释：**仅 2 条** —— `customers.customer_name` = 「真实姓名|企业名称」、`customers.customer_no` = 「身份证号码|三证合一编号」
- 含换行的列注释：**0 条**

**设计意图正确**（`propose.rs:242-247`）：首段做展示名，**完整内容留 `note`**，客户视图里 `note` 不丢信息。实测产物确认 `note` 保留了完整原文。

**但这 2 列恰好是语义被截断的场景**：「真实姓名」对一家企业客户表是不完整的名字（企业主姓名 vs 企业名本身）。影响面 2/681，**列为观察项而非缺陷**。若将来要改进，可考虑 `|` 分隔的多义写法在 `display_name` 里保留全部（`真实姓名/企业名称`）。

**一个相关的观察**：619 个属性的 `name == note`（同一个列注释既当展示名又当说明），**完全冗余**。这不算缺陷（`note` 是给人看的补充说明），但它意味着对 619/793 = 78% 的属性，客户视图把同一段文本展示了两次。

### 🟡 M12 · 客户视图属性名质量：覆盖率是买来的，145/793 属性名含拉丁技术词

覆盖率指标漂亮（75.5%），但**属性名的可读性远低于数字呈现**。实测 `out/e2e-view.json` 的 793 个属性：

| 类别 | 数量 | 样例 |
| --- | --- | --- |
| 含拉丁字符 | **145** | `license key`、`License ID`、`客户ID`、`创建者ID`、`管理员ID`、`角色ID`、`权限列表JSON`、`请求Token数`、`每天IP限制(次)`、`被代扣Open用户ID`、`rewrite赠送(万token)`、`aisearch赠送(次)`、`点击跳转的URL`、`API端点`、`SessionId` |
| **完全无中文** | **8** | `license key`、`License ID`、`processing/charged/failed`、`user quota policies in JSON array format`、`SessionId`、`0: llm, 1: emb`、`Provider ID` |
| 超 20 字符（整段枚举注释当展示名） | **34** | `是否启用：true=启用，false=禁用`、`状态(0-未开始,1-活动中,2-已结束,3-已暂停,-1-已删除)`、`类型：freeze/transfer/unfreeze/reclaim`、`操作批次ID（一次业务操作产生多条记录时关联）` |
| **裸 snake_case 技术串泄露** | **0** ✅ | S-2 **未退化**（M4 已证防线有牙齿） |

**为什么这些能通过守门器**：`MODELING_TERMS`（`view.rs:419-423`）与 `MODELING_TERMS_CJK`（`:426`）**不含** `ID` / `JSON` / `Token` / `URL` / `API` / `IP` / `Session`。所以 `客户ID`、`权限列表JSON`、`请求Token数` 全部合法通过 → `ontok ontology show` exit 0 → E2E 判「✓ 无建模术语泄露」。

**根因**：属性名**优先取源库列注释**（AC-104 的设计，`propose.rs:235-236`），而源库注释是**完全不可控的外部输入**。覆盖率从 18% 涨到 75.5% 主要靠「把注释原样当展示名」，而不是靠词表。**词表兜底只贡献了 345/964 = 36%。**

**判定**：AC-104 只考核覆盖率，形式上达标。但**这直接决定 AC-013（3 人判读）的成败** —— 客户看到「是否启用：true=启用，false=禁用」当属性名，几乎肯定会判读失败。而 AC-013 尚未执行（见 §未验证项）。这是本轮**最大的未量化风险**：报告用 8.7 属性/对象证明「投影可用」，但属性名本身的形态没被任何判据检查。

### 🟡 M13 · 判定器 AC-002 判据的两份副本已漂移，而注释自称「逐字一致」

| 文件 | 剥注释/字符串的 sed |
| --- | --- |
| `verify.sh:125` | `s\|//.*$\|\|; s\|"[^"]*"\|\|g; s\|'...'\|\|g` —— **三段**（含字符字面量剥离） |
| `verify_selftest.sh:51` | `s\|//.*$\|\|; s\|"[^"]*"\|\|g` —— **两段，缺字符字面量剥离** |

而 `verify_selftest.sh:47` 的注释明写：「与 verify.sh 的 AC-002 判据**保持逐字一致** —— 两份判据漂移本身就是一种缺陷」。

**已漂移。** 影响面小（当前 pattern 要求 `db::x`/`std::fs` 形态，Rust 的 char 字面量很难命中这些符号），**但自测测的不是 `verify.sh` 真正在跑的那份判据** —— 这与「自测要固化判据、防止判据漂移」的初衷直接相悖。按该注释自己的标准，这正属于它要防的那类缺陷。

### 🟢 M14 · 「grep 前缀脆弱性」三处确实修干净了（含新写法的一处隐患）

**修干净的结论（briefing 明确要求核实）**：

| 文件 | 写法 | 配对是否正确 |
| --- | --- | --- |
| `e2e-req003.sh:50-51` | `grep -nE ... <单文件>` + `sed -E 's\|^[0-9]+:\|\|'` | ✅ `grep -n` + 单文件 → 输出 `行号:内容`，单段式剥离**正确配对** |
| `verify_selftest.sh:103-104` | 同上（`grep -nE` + `s\|^[0-9]+:\|\|`） | ✅ 与 E2E **保持同一写法**（`:49` 注释明说要一致） |
| `verify_selftest.sh:59` | `grep -rHn '' src/domain` + `s\|^[^:]*:[0-9]+:\|\|` | ✅ `-r` 递归下 `-H` 冗余但无害，输出恒为 `文件:行号:内容`，两段式剥离**正确** |

**三处一致，无旧写法残留。** 我确认这一类（E2E 报告 §4 问题 #5 记为「同一坑第三次现身」）确实修干净了。

**新写法带来的两处口径变化**：

1. 🔴 **AC-102 的判定被悄悄放宽**。`e2e-req003.sh:43` 是 `[ "$NTAB" -ge 80 ]`，而 AC-102（`demand.md:87`）明写「判定：`jq` 断言表数 **== 91**」。**勘查到 85 张表也会判过。** 当前实测 91 属实，但判据与验收标准不一致 —— 源库少 6 张表这类真实回归不会被发现。

2. 🟡 **同一脚本内两种判定写法不一致**。step 4（`:104-108`）用 `if <pipeline>; then ok; else bad; fi` —— **正确**，`pipefail` 下管道任一段失败都会走 else。step 2/6 则是纯打印（见 B2）。同一文件里两种模式并存，说明作者知道正确写法，只是没统一。

**新写法未引入假失败**：我核对了两处 `sed` 剥离对真实源码的作用 —— `introspect.rs` 的模块文档（`:5-16`）与内联注释（`:129-137`）里确实含 `INSERT`/`UPDATE`/`DELETE` 字面量，剥离后**零假失败**（判据实测通过）。这一点 `verify_selftest.sh` 的 5 个 AC-002 用例（行首注释/行尾注释/字符串字面量/干净代码/真实 I/O）覆盖得比较扎实，是本轮判据质量最好的一处。

### 🟡 M15 · 交付物清单与实际不符：缺 `BA/verify-req003.sh`

- `demand.md:185` 交付物第 5 项：**`BA/verify-req003.sh`**
- `BA/` 下实际只有：`verify.sh`（REQ-002 的判定器）、`verify_selftest.sh`、`e2e-req003.sh`
- **没有 `verify-req003.sh`**

AC-101（写操作字面量红线）与 AC-102（`transaction_read_only=on`、表数）的判据被放进了 `e2e-req003.sh` step 1。**功能上在，但**：
- 命名与需求交付物清单不符；
- `verify_selftest.sh:18` 的使用说明仍在指引「改 `verify.sh` / **`verify-req003.sh`** 之后必须跑本脚本」—— **指向一个不存在的文件**；
- 把「跑一次 E2E」和「跑判据」耦合，意味着判据的回归检查需要一个完整的 E2E 环境（含源库连接串），这与 REQ-002 判据独立可跑的设计相悖。

**属流程缺口，非功能缺陷。**

### 🟢 M16 · 翻译表版本标记：机制正确且已接线，但无强制

**机制正确（正面）**：`ontok-propose.rs:55-57` —— `--agent-version` 缺省时取 `names.version`：
```rust
if agent_version == "unknown" { agent_version = names.version.clone(); }
```
实测确认溯源链接通：`out/e2e-proposal.json` 的 `extracted_by.version = "1.0.0"`，等于 `table_names.json` 的 `version: "1.0.0"`。**AC-107「记录具体版本」的契约是通的**，且 E2E 的调用（未传 `--agent-version`）走的正是这条默认路径。

**两处无强制**：
1. **改翻译不改 version 时没有任何机制会报**。`names.rs::load` 只校验「非空」和「不等于表名」，不校验 version 是否被更新。`names.rs` 的模块文档（`:16-20`）把「必须同时更新 version」写成流程约定，但**约定无执行**。这正是 `names.rs` 文档自己警告的场景（「否则 diff 说不清是哪次翻译变的」）—— diff 机制在，触发条件无人检查。
2. **`--agent-version` 可被调用方任意覆盖**，从而绕过翻译表版本。E2E 没传（正确），但没有任何机制阻止别人传。

**额外的溯源标签隐患**：`ontok-propose.rs:25` 的 `agent` 默认值是硬编码 `"mcode-agent"`。换一个供应商/模型跑抽取，提案里仍标 `mcode-agent` —— `extracted_by.agent` **可以被静默错标**，而这正是 AC-011/AC-014 想解决的问题（「谁在哪个版本翻的」）。两个新 bin 的注释也仍写着 `--out out/schema.json` / `out/proposal.json`（`ontok-propose.rs:7-8`），与 E2E 实际产出的 `e2e-` 前缀文件名不一致（呼应 M9）。

---

## 4. 回归检查汇总

| 检查项 | 结论 |
| --- | --- |
| `--accept-all` 与逐条路径一致性 | ✅ 同一 `set_decision`，无独立捷径，clap 互斥正确 |
| `--accept-all` 是否绕过领域校验 | ✅ **不绕过**（校验在 apply 侧，E2E step5 apply 参与判定且成功）。但 `set_decision` 本身无校验，自述措辞高估（M10） |
| `item`/`track` 改 `Option` 的报错可读性 | ✅ **无回归，且更好** —— 提示语直接给替代方案（`请给 --item <序号>，或改用 --accept-all 批量接受`） |
| 9 个新测试之间有无互相依赖 | ✅ **无依赖**。逐个变异证明它们各自独立响应（m1/m2b/m3/m4 各自只红相关测试），无顺序/共享状态问题 |
| 有无假断言（无论实现对错都过） | 词表/守门词侧**无**假断言（m1–m4 全部能红）。但 `props_dropped` 是**结构性死值**（M2），且 `cardinality` 侧**完全无断言**（B1） |
| `sanitize_comment` 切分是否切坏有意义的注释 | 🟢 影响面 2/681，且 `note` 保留完整原文；这 2 条（`真实姓名\|企业名称`）确实存在语义截断，列观察项（M11） |
| 编译警告 | ❌ **未单独统计**（见 §未验证项） |

---

## 5. 判定器与 E2E 脚本

### 5.1 `verify_selftest.sh` 是否真在测判据

实跑：`bash verify_selftest.sh` → **通过 10 · 失败 0 · exit 0**。10 个用例分布：

| 组别 | 用例数 | 是否真在执行判据 |
| --- | --- | --- |
| AC-002 领域层零 I/O | **5** | ✅ **真判据**。`run_predicate()` 构造沙箱源码 → 跑 `grep -rHn` + 完整 sed 管道 → 检查输出非空。注入 `std::fs` 能被抓，干净代码/行首注释/行尾注释/字符串字面量四类假失败源都被覆盖 |
| AC-101 勘查器只读红线 | 3 | ⚠️ **1 真 2 假**。判据 1（写操作字面量）真跑 grep；判据 2/3 是 `grep -q` 源码文本存在性（§2.3 变异 A：守卫阉割后仍报 ✓） |
| AC-105 守门词分层匹配 | 2 | 🔴 **0 真 2 假**。两个判据都是 `grep -q` 常量名（§2.3 变异 C：守门词改回子串匹配后仍双 ✓） |

**结论：10/10 是真的，但保证力远低于数字。** 5 个真判据（AC-002，REQ-002 继承）+ 4 个文本存在性 grep + 1 个真 grep。**对 REQ-003 新增的两条安全/质量保证（AC-101 的运行时只读、AC-105 的分层匹配），判定器自测提供的是虚假保证。**

这与 REQ-002 首轮的 G-1（判定器无自测）是同一类问题的**残留形态**：补了自测，但自测测的层次太浅（测「字符串在不在」而不是测「行为对不对」），于是形式上 10/10 反而更容易让人放心。

**AC 覆盖缺口（作者已主动披露，我独立确认）**：AC-102 / AC-103 / AC-104 / AC-106 / AC-107 **零自测**。E2E 报告 §5.3 与 §6 都明确写了这一点 —— **这一点应予肯定，是诚实的自我披露**。但结合 B1（AC-104/AC-106 实际不达标）、B2（AC-103/104/106/107 的判据不参与判定），这个缺口的实际代价比作者预估的大。

### 5.2 `e2e-req003.sh` 的打印式校验是否影响判定（重点）

**结论：不影响。这是阻断项 B2。** 完整证据见 §3 B2，此处只列要点：

- `e2e-req003.sh:66-88`（step 2，9 项）与 `:127-142`（step 6，avg/零属性）是 python heredoc，**只 print**；
- python 无法改 shell `FAIL`；脚本 `set -uo pipefail` 无 `-e`；`exit $FAIL`（`:162`）；
- **实测**：喂入 8 项全错的合成数据 → 8 个 `✗` + **python 退出码 0** → 脚本判通过；
- **受影响的不参与判定的判据**：AC-103 的 `display_name` 中文性 / 属性数 / 关系两端 / key 唯一，AC-104 的 avg≥3 / 零属性=0，AC-107 的 `extracted_by` 版本与只读留痕；
- **部分缓解**：悬空关系由 `ontok-propose.rs:88-90` 的 `bail!` 间接拦住（`e2e:65 || { bad; exit 2; }` 参与判定）。

**确实参与判定的检查**（如实列出，避免夸大）：`:31`（二进制就绪）、`:37`（勘查失败）、`:39`（`transaction_read_only=on`）、`:43`（表数 ≥80）、`:50-55`（写操作字面量红线）、`:58-59`（建项目/版本）、`:65`（抽取失败，间接含悬空>0）、`:91`（导入失败）、`:104-108`（批量 decide）、`:112`（整体通过）、`:115-116`（apply/冻结）、`:122-126`（守门器未拦截）、`:154`（冻结后拒写）。

**所以 E2E 的「守门器未拦截」判定是有效的**（`:122` 检查 `ontok ontology show` 的退出码）—— 这也解释了为什么 B1 的 `N:1` 能一路绿灯：它既不在守门词表里，守门器的退出码又是真在检查的，只是检查不出这个形态。

### 5.3 三处判据脆弱性是否修干净

**已修干净 ✅** —— 详见 §3 M14 的逐处配对核对表。三处（`e2e-req003.sh:50-51`、`verify_selftest.sh:103-104`、`verify_selftest.sh:59`）写法各自正确且内部一致，无旧写法残留。这是本轮判据工作做得最扎实的一块。

**但引入了两处新的口径问题**：
1. 🔴 `e2e-req003.sh:43` 的 `-ge 80` 与 AC-102 的「== 91」不一致（放宽了验收标准）。
2. 🟡 step 4 用 `if pipeline`（对）与 step 2/6 用纯打印（错）并存，同一文件两种模式。

**新写法未引入假失败**：`introspect.rs` 的文档注释含 `INSERT`/`UPDATE`/`DELETE` 字面量，剥离后零假失败（实测通过）。

---

## 6. 我没能验证的部分

以下是我**明确未完成或无法完成**的检查，不做任何推测性结论：

| # | 未验证项 | 原因 | 要确认它需要什么 |
| --- | --- | --- | --- |
| 1 | **`introspect()` 对真实源库的端到端行为** | **无法访问源库**（连接串由用户提供、不入库，`.env` 内无）。`out/` 只有已产出产物 | 一条只读的源库连接串（`ONTOK_SOURCE_DATABASE_URL`），或在源库上跑一次 `ontok-introspect` |
| 2 | **E2E 脚本全流程实跑**（`bash e2e-req003.sh`） | 同 #1，脚本第 14 行 `: "${ONTOK_SOURCE_DATABASE_URL:?...}"` 会直接退出 | 同 #1。**我改为逐段验证**：step2/step6 的校验块用合成数据实跑（§5.2），step 1 的只读语义用测试库探针等价复现（§1.1） |
| 3 | **E2E 报告 §0 表格里的其余步骤**（1.5 建项目 / 3 导入 / 5 apply+冻结 / 7 diff / 8 冻结后拒写） | 依赖完整 E2E 环境 | 同 #1。这些步骤的**判据代码**我已审（`:58-59`、`:91`、`:115-116`、`:145`、`:154`），确认参与判定；**但其实际执行结果未复现** |
| 4 | **词表覆盖率 546/1051 = 51%（自述 #6）** | 探针测试与变异 harness 争用同一源码目录（harness 运行期间源码处于变异态）。为不污染已完成的变异结果，我**主动放弃**了这次复算 | 独立跑一次只读探针（`project()` 对 1051 列强制走词表兜底，统计留存数）。**注：端到端数字 793/1051=75.5% 我已独立算出并采信**；未复算的只是 546 这个中间值 |
| 5 | **「零编译警告」（自述 #8）** | 未单独跑 `-W` 统计。我的测试构建未见 `warning:` 输出到失败，但**没有系统性统计过警告数** | `cargo build 2>&1 \| grep -c warning` / `cargo clippy` |
| 6 | **AC-013 客户视图 3 人判读** | 按 briefing 要求**不判** —— 需 3 名**非开发者**外部评审人 | 组织外部评审。**但我已指出 M12 会显著影响它的结果**：8 个属性名完全无中文、34 个超 20 字符、145 个含拉丁技术词。建议**在组织判读前先处理 M12**，否则大概率浪费 3 名评审人的时间 |
| 7 | **`M6b`（`family_high_risk_records`→「同设备高风险记录」）是否真错** | 全表 11 列无任何 device 依据，但「family」的真实业务含义需业务知识 | 业务方确认 `family` 的含义。**标注为「存疑」而非「错译」** |
| 8 | **本体库并发写入下的测试稳定性** | 测试期间出现 1 次 `test_apply` 失败，我判定为环境噪声（30432 代理重置连接）并做了重跑确认（3/3 通过 + m6b 复现），但**未做多次全量套件重复运行**来量化整体 flake 率 | 在低并发时段连跑 3–5 次全量 `cargo test`，统计 flake 率 |
| 9 | **`--accept-all` 在含 G-5 场景（同轨 Rejected 被抹除）下的行为** | 需构造 G-5 场景；且 G-5 按需求 §6 是「待产品拍板，不擅自决定」 | 产品先对 G-5 拍板 |

---

## 7. 结论

### VERDICT：**不通过**

**理由**：交付物在**正确性**上大体站得住 —— 我实测确认只读保证的第 2/3 层真实有效（层 1 无效这点作者已诚实披露）、抽取产物与自述数字逐项对得上、翻译表 91↔91 精确且 0 重名 0 漏译、9 个新测试里 4 个经变异证明有真实判别力。**但它在一个决定性指标上不达标，且这个不达标被两套「全绿」信号掩盖了**：

- **B1**：客户视图 **51/51 条关系**带原始基数记号 `N:1`（AC-005 明令禁止的建模术语），守门器对其结构性失明，而 **m5 证明测试对抽取器实际发货的基数语义零敏感**（51 条关系基数全反，157/157 绿）。
- **B2**：E2E 脚本里 **AC-103/104/106/107 的量化判据只打印不判定**（实测 8 项全 ✗ 仍 exit 0），报告却把「9 项校验全过」列为通过项。

两者叠加的后果是：**「148 测试全绿 + 判定器 10/10 + E2E 全部通过」这三个信号，在本轮全部无法覆盖核心交付质量。** 这与 REQ-002 首轮「133 测试全绿 + 判定器 exit 0 却有 3 个阻断项」是同一种失效模式，只是这次发生在**投影输出的可读性**而非落库安全上。

### 阻断项（必须修复才能通过）

| # | 阻断项 | 位置 | 最小修法 |
| --- | --- | --- | --- |
| **B1** | 客户视图泄露 `N:1` 原始基数记号，51/51 条 | `src/domain/view.rs:269-276`（`cardinality_cn` 缺 `N:1`） | 补 `N:1` 分支；**并加一条契约测试**：`REL_SUFFIXES` 的每个 cardinality 字面量必须能被 `cardinality_cn` 翻译（钉死生产者-消费者两端的差集，否则下次加记号又会漏） |
| **B2** | E2E step2/step6 校验只打印不参与退出码 | `BA/e2e-req003.sh:66-88`、`:127-142` | 让 python 以退出码表达结果（`sys.exit(1)` if any fail）并用 `\|\| bad` 接住；或把判定搬回 shell。同时把 E2E 报告 §0 的表述改成与实际判定一致 |

### 必须同批修（不阻断本需求，但与阻断项同源、且成本极低）

| # | 项 | 理由 | 修法 |
| --- | --- | --- | --- |
| 1 | **AC-101 两层安全保证零测试覆盖 + 判定器对 fail-closed 假通过**（m6/m6b/m7 全绿；变异 A 5/5 假通过） | 只读保证是全平台唯一接触客户数据的代码；现状正确但**无任何机制防回归**，与 REQ-002 B1 同型 | ① 把 `verify_selftest.sh:112-116` 的 fail-closed 判据从「`!= "on"` 字符串存在」改成**行为判据**（例如：注入一个让 `current_setting` 返回非 `on` 的连接，断言 `introspect()` 返回 Err）；② `verify_selftest.sh:125-134` 的 AC-105 两条改成**行为判据**（断言 `scan_modeling_terms` 在前缀+建模术语形态下仍命中）；③ 补 m7 那样的「阉割守卫」负向用例 |
| 2 | **守门词白名单越界**（M1，10 个形态实测无命中） | 与 B1 同向：都是守门器在真实形态上的盲区，而这是 AC-005/AC-106 的最后一道闸；属性名来自不可控的源库注释 | 白名单豁免只作用于 `key`（必要时含 `pk`）；补「白名单前缀 + 其它建模术语仍被抓」的测试 |
| 3 | **`force_read_only` 不覆盖 `=off` + 该形态无单测**（M3） | 契约违背；当前被「层 1 无效」掩盖，sqlx 一修复即成真漏洞 | `starts_with` 命中时也覆盖为 `=on`；补 `already-off` 单测 |
| 4 | **`force_read_only` 调用点零覆盖**（m8：删调用 148/148 绿） | helper 有单测但接线没测 —— helper 可以是死代码而全绿 | 加一条断言 `introspect()` 确实走了 `force_read_only` 的测试（或让 `force_read_only` 接受连接串并返回可断言的结构） |
| 5 | **`props_dropped` 死统计**（M2） | 装饰性防线，出现在 E2E 报告的证据表里 | 要么接上（`project()` 返回丢弃数），要么从 `ExtractionStats` 删掉，别在证据表里放一个恒为 0 的数 |
| 6 | **AC-102 判定被放宽为 `-ge 80`**（M14） | 验收标准写的是 `== 91`；源库少 6 张表不会被发现 | 改回 `== 91`（或把 AC 文本改成 `-ge 80` 并说明理由） |
| 7 | **AC-104 报告口径自相矛盾**（M8） | 报「51%」配判「✅ 超额达标」，而 51% < 60% 目标；真正达标的 75.5% 从未计算 | 报告里补上端到端 75.5%（793/1051），并说明 51% 是词表单独覆盖率；顺带解释 1068 vs 1051 |
| 8 | **`out/proposal.json` / `out/extract-stats.json` 是陈旧产物**（M9） | 与当前代码形状不符，且 `proposal.json` 会被 `ontok propose import` 拒收 —— 审阅者拿到的是误导性证据 | 删除或重新生成；`ontok-propose.rs:7-8` 的注释文件名与 E2E 实际产物对齐 |
| 9 | **交付物缺 `BA/verify-req003.sh`**（M15） | `demand.md:185` 明确要求；`verify_selftest.sh:18` 指向不存在的文件；判据与 E2E 耦合导致回归检查需要完整环境 | 抽出独立判据脚本，或修正 `demand.md` 与 `verify_selftest.sh` 的指引 |
| 10 | **AC-002 判据两份副本已漂移，而注释自称逐字一致**（M13） | 自测测的不是 `verify.sh` 真正在跑的判据 —— 正是该注释要防的缺陷 | 把 `verify.sh:125` 的三段 sed 与 `verify_selftest.sh:51` 对齐；更好的做法是让自测**直接调用 `verify.sh` 里的判据函数**而非复制 |
| 11 | **翻译表错译 `customer_interests`→「客户意向」**（M6） | 列注释（权益id/购买时间/到期时间/已激活）证明这是「客户已购权益」表；「客户意向」= 销售线索，严重误导 | 改为「客户权益」/「客户已购权益」；顺手复核 `interest_configs`→「利息配置」是否也应统一（同一词两种译法必有一错） |
| 12 | **属性名质量未被任何判据检查**（M12） | 覆盖率靠「把源库注释原样当展示名」买来；8 个属性名完全无中文、34 个超 20 字符。**这直接决定 AC-013 成败，而 AC-013 尚未执行** | 在 AC-104 判据里加一条「属性名可读性」检查（中文占比 / 长度上限 / 技术记号黑名单含 `ID`/`JSON`/`Token`/`URL`/`API`/`IP`）。**建议在组织 AC-013 判读前先做这个**，否则大概率浪费 3 名评审人 |

### 建议但不属于本轮范围

- **关系推断质量**（M4/M5）：51 条关系只覆盖 31% 的约定列，漏的包含 6 条指向 `llm_model` 的真关系（注释明写目标）和 7 条 `main_customer_id`；同时 `*_ids` 分支与 `singular_to_table` 的复数规则在本 schema 上是死代码。建议按「词干 ≠ 表名是主要失败模式」重新设计（可利用已有列注释做目标表消歧 —— 注释里直接写着「对应 llm_model.model_id」）。另外 41/51 指向 `customers`，把客户/账号/主账号/子账号/用户压平了，应引入主体类型区分。**但这属零外键环境的固有限制，需求 §5.1 已预告，且作者在 E2E 报告 §6 主动列了「关系准确率需人工抽查」—— 态度可取，不宜作为本轮阻断项。**
- **`sanitize_comment` 对 `真实姓名|企业名称` 的截断**（M11）：影响 2/681，可留待后续。
- **翻译表版本的强制机制**（M16）与 **`extracted_by.agent` 硬编码**（M16）：溯源链已通，但约定无执行、可被静默错标。

