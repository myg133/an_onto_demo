# REQ-108 · 第 4 轮终验（R5 封口复验）· 合并判定

> **审核对象**：`feature/REQ-108` = **`d4e3c68`**（基线 `develop` `4573269` + **10 个** `[Dev]` 提交）
> **被验树**：HEAD `d4e3c68dde25f7db1136f6da7f2494b4311caf1a`，tree **`47194bcab67068aa9f8c2812b2654706bae426ec`**
> 　　（独立 worktree `.review-workspace/req108/qa3` detached 在此提交，tree 与 feature tip **逐位相同**；feature 分支零改动）
> **审核方式**：另建 worktree + 自己从该树编的二进制重跑；DB 事实用 `psycopg2` 另查；
> 攻击全部走 **CLI 真进程**（不是读代码推断），逐条记录 **exit code + 文案首行**。
> **审核时刻**：2026-10-21 17:37–17:46（+08:00）；源库快照 = 该时段的 `anspire_open_uat`
> **本轮作业单**：`acceptance.md` §六 A-13；P0-1 ~ P0-4
> **上轮报告**：`qa-review-2.md`（R5 的原始复现方式，本轮按同样手法再打）

---

## 0 · 结论

**PASS 带非阻断（新增阻断 0 条）→ 可以合并。**

| 本轮要回答的问题 | 我的独立结论 |
| --- | --- |
| **R5 是否真封**（A-13 的 4 条判据） | ✅ **真封**：原样攻击链 `exit 1`；反向 `exit 1`；`--no-sample` 真产出 `exit 0`；真 `proposal.json` 148 条 `exit 0` |
| **有没有同类新洞**（删/伪造证据让高分通过） | ✅ **没有新的**：三种变体里 2 种被封（`exit 1`）；唯一放行的是**伪造 `sampled/matched` 的 5/5**——这正是 A-12 / A-16 已登记的 **R2**，不是新洞 |
| **不退化** | ✅ **全部成立**：9/9 拒绝、4 份产物两次运行 sha256 相同、客户视图 0 泄漏且两个不同分数项目逐字节相同、`--lib 230` / `--doc 4` / **32 套件 313**、六红线 + `tools.rs` + `guard()` + `property_filter.rs` diff 空 |
| **文档 N6/N7/N8** | ✅ `verification-report.md` §11 **真实存在**（L616–L803），32 套件名单与我实测**逐套件对上 313**，`source_enumeration` 已在名单内且实跑 10 passed |
| **能不能合并** | ✅ **可以合并**；R2/R3/R4 仍按 A-12/A-16 登记，⛔ 不要写成「已封堵」 |

**一句话**：本轮修的是「删掉最强的采样证据仍然能拿高分」；这条**已经封住**。剩下的唯一通路是「把 `sampled/matched` 一起改成 5/5 再编一个自洽分数」——BA 在 A-16 已明确这类「伪造全套自洽证据」只登记、不阻断。

---

## 1 · 我验的确实是那棵树（不是「看起来像」）

```
feature tip d4e3c68  tree = 47194bcab67068aa9f8c2812b2654706bae426ec
qa3 worktree HEAD    tree = 47194bcab67068aa9f8c2812b2654706bae426ec   ⇒ 同一棵树
qa3 git status       = 空（未跟踪的日志都在 .review-workspace/req108/qa3-out/，不在树里）
feature-REQ-108      = 空 status；10 个提交；无 origin/feature/REQ-108（未 push）
```

**二进制（我自己在 qa3 里从该树编的，`CARGO_TARGET_DIR=.review-workspace/req108/qa3-target`）**：

| 二进制 | sha256 |
| --- | --- |
| `ontok.exe` | `63F08619FA034402DEE31F9EBECA24385EDED2C6C987ACD901553685DB485C7A` |
| `ontok-propose.exe` | `19CCC57130C4E5CD08F7DB444C9EF81C506281B97B5BF05D87389806A11E9A90` |
| `ontok-introspect.exe` | `63B1BA2A60E33B0C647805ED1D8EEF7B2EC0871A2D26ECBFD2F5F35FE59131A` |

**环境**：

| 项 | 事实 |
| --- | --- |
| 本地库 | `postgresql://postgres:***@172.17.0.87:5432/ontok_demo`；**未复用** Dev 的 `qa108run` / `r5fix108` / `r5apply108b`，本轮新项目：`qa108r4run`（产物）/ `qa108r4att1`、`qa108r4att2`、`qa108r4gate`（攻击）/ `qa108r4ns`（`--no-sample`）/ `qa108r4real`（真提案）/ `qa108r4forge`（R2 全链路）/ `qa108r4honest`、`qa108r4tamper`（视图）/ `qa108r4apply2`（apply 第二道闸） |
| 源库 | `172.16.120.20:30432/anspire_open_uat`，连接串只从环境变量来；introspect 自报 `transaction_read_only = on` |
| 源库快照 | **96 张基表 / 1116 列 / 0 外键 / 95 表单列主键**；勘查时刻 `2026-10-21T09:38:12Z`；与上一轮 `schema-src.json` **去掉 `introspected_at` 后逐字段相同**（无漂移） |
| 绕行 | 跑 `cargo test --test` / `--lib` / `--doc` 时把 `src/bin/ontok-gate-preflight.rs` 临时挪到 `.review-workspace/req108/parked/`，每轮跑完挪回；sha256 前后均 `C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D`（identical=True） |

---

## 2 · P0-1 · A-13 的 4 条判据（逐条，真 CLI + 真库）

攻击文件由 `run1/proposal-rejected.json` 的 **E5**（`llm_model_sort.model_id → llm_model`，0.20 / rejected / `zero_hit` / 5 行 0 命中）派生；每条只改一处细节，项目名与条目 key 换成独立值，避免与 Dev 的库状态互相污染。

| # | 输入（我构造的） | 期望 | **实测 exit** | 实测文案首行 | 判定 |
| --- | --- | --- | --- | --- | --- |
| 1 | `score 0.99` + `band high` + `data_evidence: sampled` + **4 个结构信号**（`name_match`/`type_compatible`/`target_is_key`/`not_all_null`），**无 `sample_hit_rate`、无 `veto`**（A-13 原样攻击链） | ≠0 | **1** | `✗ item 1 的 confidence 不合法：confidence.data_evidence = sampled —— 声明了采样证据但 signals 里没有 sample_hit_rate：数据证据（sampled/matched/digest）只挂在这条信号上，删掉它之后剩下的结构信号不可复核（A-13 · R5）` | 🟢 **封住** |
| 2 | `data_evidence: none` + **带** `sample_hit_rate`（5/5，value 1.0） | ≠0 | **1** | `✗ item 1 的 confidence 不合法：confidence.data_evidence = none（声明没采过样），但 signals 里出现了 sample_hit_rate —— 声明没有采样证据却又摆出一份采样证据，二者矛盾（A-13 · R5）` | 🟢 **封住** |
| 3 | **`--no-sample` 的真产出**：取 `qa3-out/nosample/proposal-pending.json` 里的一条关系（`data_evidence: none` + `name_match`/`type_compatible`/`target_is_key` 三个结构信号，0.45/low），单条导入 | **=0** | **0** | `提案 p1 已导入，共 1 个条目：` / `[1] add_link_type →activity` | 🟢 **不误伤** |
| 4 | **真 `proposal.json`**（`qa3-out/run1/`，148 条 = 96 实体 + 52 关系，每条关系 5 信号） | **=0** | **0** | `提案 p1 已导入，共 148 个条目：` / `[1] add_entity_type`；读回 `置信度（evidence-scorecard-v1）：高 44 · 中 8 · 低 0 · 拒绝 0（共 52 条关系）` | 🟢 **放行** |

> 判据 4 的「52 条关系 × 5 信号」不是看顶层声明：我逐条数了 `run1/proposal.json` 的 `add_link_type` 条目，
> 有 `confidence` 的 **52 条**，`signals.len()` 的集合是 **`{5}`**，`data_evidence` 全部 `sampled`。
> 判据 3 也不是我手拼的形状：它是 `ontok-propose --no-sample` 的真实输出（82 条关系全部 `none` + 3 信号）。

**结论**：A-13 的 4 条判据**全部按预期**。R5 的攻击链在 **import 第一道闸**就被打回，且文案点名缺失的那条信号，可顺着文案定位。

---

## 3 · P0-2 · 三种「删/改证据」变体（不穷举）

> 说明：题目建议的「`sampled/matched` 与**顶层**不一致」在本实现里**不可表达**——
> `Confidence` 顶层没有第二份 `sampled/matched`，聚合量只存在于 `signals[sample_hit_rate]` 上。
> 所以我按同一语义选了三个**能真实构造**的最近变体：伪造聚合量、整段删 `data_evidence`、`value` 与查表值不一致。

| # | 变体（相对 E5 改了什么） | 期望 | **实测 exit** | 实测文案首行 / 结果 | 接受？ |
| --- | --- | --- | --- | --- | --- |
| **A** | **伪造全套自洽证据**（R2 形状）：`data_evidence: sampled`，把 `sample_hit_rate` 的 `sampled/matched` 改成 **5/5**、`value` 改成 **1.0**，`score 0.99 / high`，**删 `veto`**；`sample_digest` **保留原 0/5 的 digest**（未一起伪造） | （R2 形状，预期放行） | **0** | `提案 p1 已导入，共 1 个条目`；随后我把它**追加进 148 条真提案**走全链路：`import 149` → `decide 149×2 accepted` → `apply → v2`；库里该关系 `confidence = 0.99`，`confidence_signals = {score:0.99, band:high, sample_hit_rate: 5/5, value:1.0, digest: 64c14f73…}` | ⚠️ **接受（A-16 登记，非阻断）** |
| **B** | **整段删 `data_evidence` 字段** + 删 `sample_hit_rate`（4 个结构信号，0.99/high，无 veto） | ≠0（缺失按 `sampled` 严格侧） | **1** | 与 P0-1 #1 **同一文案**：`… confidence.data_evidence = sampled —— 声明了采样证据但 signals 里没有 sample_hit_rate …（A-13 · R5）` | 🟢 **接受（封住）** |
| **C** | 有 `sample_hit_rate`，但 `sampled=5 / matched=0` 而 `value=1.0`（查表值应为 0.0），`score 0.99/high`，无 veto | ≠0 | **1** | `✗ item 1 的 confidence 不合法：signals 自相矛盾：sample_hit_rate 的 value = 1 与 5/0 的查表值 0 对不上` | 🟢 **接受（封住）** |

### 3.1 A 的归类：**R2，不是新洞**

- 它能过的原因**不是**「R5 没修好」，而是**证据本身被伪造得自洽**：`sampled/matched/value` 三者互相一致，`data_evidence` 一致，权重一致，分数也自洽；
- 这与 A-12 / A-16 描述的 **R2「伪造全套自洽证据」逐字对应**（连 `sampled/matched` 一起改、再算一个自洽分数）；
- 我**故意保留**了原 0/5 的 `sample_digest`（`sha256:64c14f73…`）：本地没有任何校验把 digest 与 `matched` 绑起来，所以它不挡；但 digest 是**回源复采的锚点**——拿它回源库重算命中数就能证伪。即使把 digest 一起改，也只是同一登记项（A-16 已写明「要连 digest 一起伪造」）；
- 威胁模型上，这需要**手改提案文件并主动伪造数据证据**（恶意篡改），不是诚实错误：诚实生产者永远由 `score()` 生成 `sampled/matched/value/digest`，不会产出这种形状。

⇒ **按 A-16 判：接受、登记、不阻断**。⛔ 交付物里不能写成「R2 已封堵」。

**B / C 说明删证据类没有新缺口**：删整段 `data_evidence`、换写法留空壳（本修的 `sampled/matched` 齐全性检查）、改 `value` 都被挡住；**唯一放行的是伪造聚合量**，而它属于已登记的 R2。

---

## 4 · P0-3 · 不退化（全部本轮重跑，不引用 Dev 数字）

### 4.1 真实库：9 条已知错误 **9/9 仍全部 rejected**

`cargo test --test test_req108_live test_req108_ac006_nine_known_errors_all_rejected -- --nocapture`
（真源库，8.05 s；日志 `qa3-out/live-ac006.log`）：

| # | 关系 | score | band | veto | sampled/matched | produced |
| --- | --- | --- | --- | --- | --- | --- |
| E1 | `balance_operation_log.batch_id → gift_code_batch` | **0.20** | rejected | `zero_hit` | 5/0 | true |
| E2 | `instance_manager.session_id → simulate_login_sessions` | **0.20** | rejected | `zero_hit` | 5/0 | true |
| E3 | `internal_operation_records.batch_id → gift_code_batch` | **0.20** | rejected | `zero_hit` | 3/0 | true |
| E4 | `llm_model_peak_config.model_id → llm_model` | **0.20** | rejected | `zero_hit` | 4/0 | true |
| E5 | `llm_model_sort.model_id → llm_model` | **0.20** | rejected | `zero_hit` | 5/0 | true |
| E6 | `llm_provider.model_id → llm_model` | **0.20** | rejected | `zero_hit` | 5/0 | true |
| E7 | `llm_provider_groups.model_id → llm_model` | **0.20** | rejected | `zero_hit` | 5/0 | true |
| E8 | `dim_customer_binding.channel_code → marketing_channel` | **0.15** | rejected | `type_incompatible` | 5/0 | **false** |
| E9 | `marketing_channel_customer.channel_code → marketing_channel` | **0.15** | rejected | `type_incompatible` | 5/0 | **false** |

- 产物侧同样成立：E1–E7 都在 `run1/proposal-rejected.json`（`0.20 / rejected / zero_hit`），**7/7 digest 与上一轮逐位相同**
  （`1deb6586…` / `b67e5a08…` / `f0b94ee5…` / `a6177f79…` / `64c14f73…` / `81184b2d…` / `c38b3299…`）；
- E8/E9 在三份产物里**都不存在**（`proposal.json` / `proposal-pending.json` / `proposal-rejected.json` 都没有这两个 key），
  按 A-6 用**构造复算**得 `0.15 / type_incompatible / produced=false`——如实区分「不产出」与「产出且拒绝」，没有混写。

### 4.2 两次 CLI 运行：4 份产物 sha256 **逐字节相同**

新项目 `qa108r4run`，同一 schema、同一源库、同一二进制连跑两次（`run1` / `run2`）：

| 文件 | run1 vs run2 | sha256 |
| --- | --- | --- |
| `proposal.json` | ✅ IDENTICAL | `959CA45544C1A485BBAFB47115E2002E77A4E99781EB2FD154C028912AC112BB` |
| `stats.json` | ✅ IDENTICAL | `F1E6735511C579B8211A114CC5C8ED6D82472FBD6FFF7CC775913C5EA1D06560` |
| `proposal-pending.json` | ✅ IDENTICAL | `07280C141B6E5F891A0B5DA9A7D7AE2DD7229EA6CDD843B33FC24CECE71B7BC3` |
| `proposal-rejected.json` | ✅ IDENTICAL | `A48F365FA4B75A8A745E4201E00A3B3BFB2C9CDF3D4B54AA998618C102108697` |

> `stats.json` 与上一轮、与 Dev 报告的哈希完全相同；`proposal.json` 的哈希与上一轮不同，差异只来自**顶层 `project` 字段**
>（`qa108run2` → `qa108r4run`）。我另外做了**逐条 deep-equal**：`proposal` / `pending` / `rejected` 三份的 `items`
> 与上一轮**完全相同**，`stats` 也完全相同 ⇒ 评分、路由、证据链**一个字节没动**。

### 4.3 客户视图：grep = 0；两个不同分数项目的视图**逐字节相同**

新建 `qa108r4honest` / `qa108r4tamper`，喂**同一份** 148 条提案；只把一条 high 关系的 `score` 从 `1.0` 改成 `0.99`（其余字段不动，自洽），两边都 apply 出 v2：

| 判据 | 结果 |
| --- | --- |
| 库内快照 | 诚实项目该关系 `confidence = 1.0`；篡改项目 `confidence = 0.99`（**差异真实存在**） |
| 客户视图 JSON（`ontology show`）grep `confidence` / `score` / `band` | **0 命中**（59,462 B，两个项目都是） |
| 客户视图 HTML（`--html`）同上 | **0 命中**（34,848 B，两个项目都是） |
| **两个分数不同项目的客户视图** | `customer.json` **逐字节相同**：`09CA98BFD03778268EDE7BD393F4DCF074F980104140EE7AAD44AA4911D95D19`；`customer.html` **逐字节相同**：`4B96D51CA9AAE110F00C886C9F026735979AD38551C846FEB4E4C57D22C852A5`（HTML 与上一轮同哈希） |
| `--model-view` | `"confidence"` **52 次**（52 条关系每条都有） |

> 最后一条比 grep 更硬：**改了分数，客户视图一个字节都不动** ⇒ 客户视图根本不读这条通路，不是「恰好没打印」。

### 4.4 `--lib` / `--doc` / 32 套件（本轮实测，逐套件）

| 目标 | 我实测 | 报告 §11.3 / status.md | 判定 |
| --- | --- | --- | --- |
| `cargo test --lib` | **230 passed / 0 failed** | 230 | ✅ |
| `cargo test --doc` | **4 passed / 0 failed** | 4 | ✅ |
| 13 个 REQ-107 出口套件 | **70 passed / 0 failed** | 70 | ✅ 逐套件与名单一致 |
| 16 个邻域套件 | **218 passed / 0 failed** | 218 | ✅ |
| `test_req108_live` | **5 passed / 0 failed**（真源库，22.18 s） | 5 | ✅ |
| `test_req108_confidence` | **10 passed / 0 failed** | 10 | ✅ |
| `test_req107_source_enumeration`（N8 新增） | **10 passed / 0 failed** | 10 | ✅ |
| **32 套件合计** | **313 passed / 0 failed** | 313 | ✅ **逐套件数字全部对得上** |

- 32 个套件的**逐套件 passed** 与报告 §11.3 表格**每一位都相同**（我另写脚本解析 `test result:` 行核对，`mismatch = {}`）；
- 分组账：`70 + 218 + 5 + 10 + 10 = 313`，`13 + 16 + 1 + 1 + 1 = 32`；
- `src/bin/ontok-gate-preflight.rs` 的 sha256 前后都是 `C7CF95E2…571322D`，`identical=True`。

### 4.5 红线 / 禁止项 / 依赖

| 项 | 结果 |
| --- | --- |
| `tests/drop_rate.rs` | `git diff 4573269..d4e3c68` **空** |
| `tests/test_fk_naming.rs` | **空** |
| `src/domain/view.rs` | **空** |
| `src/domain/review_view.rs` | **空** |
| `src/mcp/tools.rs`（含 `guard()`，L693） | **空**（`guard()` 函数体零改动） |
| `src/domain/property_filter.rs`（G1–G4 措辞） | **空** |
| `Cargo.toml` / `Cargo.lock`（新依赖） | **空**（零新 crate） |
| 删测试 / 新增 `#[ignore]` | **0 / 0**（diff 里唯一一处 `#[ignore]` 是注释文字「不用 `#[ignore]`」） |
| 被删除的测试函数 | **0** |
| 提交数 | `4573269..d4e3c68` = **10 个** `[Dev]` 提交 |

### 4.6 顺带复核：apply 侧第二道闸仍有效（§11.2 的声明）

在 `qa108r4apply2` 导入真 148 条提案后，用 SQL 把 **seq=97** 那条关系的 `payload.confidence` 改成 P0-1 #1 的攻击形状，再走同一条链：

| 步骤 | exit | 结果 |
| --- | --- | --- |
| `review decide --accept-all` | **0** | 148 条 × 2 轨 → accepted（decide 只改状态） |
| `proposal apply` | **1** | `应用失败：关系条目的 confidence 不合法：confidence.data_evidence = sampled —— 声明了采样证据但 signals 里没有 sample_hit_rate：…（A-13 · R5）` |
| `version list` | — | 只有 `v1`，**没有 v2**（快照没有被写坏） |

⇒ import 与 apply 共用 `from_untrusted`（`src/db/proposal.rs:109` / `src/domain/apply.rs:309`），没有第二份判据。

### 4.7 入口不退化（上一轮的两条阴性/阳性对照）

| 文件 | exit | 结果 |
| --- | --- | --- |
| `run1/proposal-pending.json`（20 条低档，`importable: true`） | **0** | `提案 p1 已导入，共 20 个条目` |
| `run1/proposal-rejected.json`（10 条，`importable: false`） | **1** | `顶层声明 importable = false —— 这是被拒绝档，不能导入。` |

---

## 5 · P0-4 · 文档核对（N6 / N7 / N8）

### 5.1 `verification-report.md` §11 **真实存在**，内容对得上

文件 803 行，`## 11 · 第 3/4 轮（A-10 / A-13）与测试口径（N6 / N7 / N8）` 在 **L616–L803**：

| 小节 | 内容 | 核对结果 |
| --- | --- | --- |
| §11.1 | 第 3 轮 R1 修复（`confidence.rs:930/945`）+ `confidence: null` 裁定 + `4acbf5a` 上 t5 本来就红的说明 | ✅ 与代码/上一轮复现一致 |
| §11.2 | 第 4 轮 R5 修复点（`confidence.rs:796`）+ 两条判据表 + **4 条判据实测** + 变异自证 + apply 第二道闸 | ✅ 4 条实测与我本轮结果**逐条同 API**（exit 1/1/0/0）；apply 闸我独立复现（§4.6） |
| §11.3 | `lib 230` / `doc 4` / **32 套件 313** + 逐套件名单 + 302/307 的解释 + bin 绕行 sha256 | ✅ 32 行逐套件数字与我实测**全部相同**，`70+218+5+10+10=313` |
| §11.4 | 已知限制：R5 已封；R2/R3/R4 **仍登记、不写成已封堵** | ✅ 与 A-12/A-16 一致 |
| §11.5 | N6/N7/N8 逐条处置表 | ✅ 三条都有落点 |

### 5.2 回归集名单能逐套件对上 **313**

报告 §11.3 的回归名单共 **32 个套件**：13（出口，70）+ 16（邻域，218）+ `test_req108_live`（5）+ `test_req108_confidence`（10）+ `test_req107_source_enumeration`（10）= **313**。
我按名单逐个 `cargo test --test <name>`，解析 `test result:` 得到 **313 passed / 0 failed**，**每一套件的 passed 数与表格完全相同**（脚本核对 `mismatch = {}`）。
「307」的旧口径在报告与 `status.md` 里都已声明作废（`70+218+9+10`，即用 `source_enumeration` 换掉 `live` 的 5 条）。

### 5.3 `source_enumeration` 是否真的进了回归集

- 报告 §11.3 表里以 **★ N8 新增** 单列，10 passed；
- `status.md` 的「32 套件 313」名单包含它；
- 我在本轮 32 套件回归里**真的跑了它**（`10 passed / 0 failed`），不是只写在文档里；
- 这也把上一轮 N8 的覆盖盲区（「出口文件每个函数都登记」）纳入了标准回归。

> 边界说明（**不构成新问题**）：本仓没有 CI 配置文件，「回归集」目前是一份**文档名单**，靠流程执行；
> 本轮已把该名单逐套件跑通，数字可复核。若将来要机器强制，需要另立 CI 任务——不在本 REQ 范围。

---

## 6 · 阻断项 / 非阻断项

### 阻断项：**0 条**

- A-13 的 4 条判据全部符合预期；
- 删证据类的三种变体：2 种被封，1 种是**已登记的 R2**（伪造全套自洽证据），不是新洞；
- 9/9、确定性、客户视图、32 套件 313、红线、apply 第二道闸全部成立。

### 非阻断项：**0 条新增**（R2/R3/R4 均为 BA 已登记，不重复计）

| # | 项 | 状态 | 说明 |
| --- | --- | --- | --- |
| **R2** | 伪造 `sampled/matched` 5/5 并算自洽分数 | **仍在**（A-12 / A-16 登记） | 本轮实测复现（P0-2 A）：可导入、可 apply 到 v2。需要连 `sample_digest` 一起伪造/或回源复采才能证伪；防的是诚实错误，不是有 DB 写权限的恶意篡改 |
| **R3** | 3 条结构性 veto 在 signals 里无对应信号 | **仍在**（A-12 登记） | 只压低分数，本轮未发现新的抬分通路 |
| **R4** | 未实现「按权重重算 score 必须相等」 | **仍在**（A-12 登记） | 权重 + veto + R5 锁死后，单改 `score` 已过不了闸；重算属于加固，可后续 |

> 另外一条**边界**（不是新问题，也不建议本轮动）：校验器是**结构校验**，不要求 5 个信号齐全，也不重算分数（R4）。
> 因此「少写一个 `type_compatible` 信号、最高 0.90/high」在形状上仍可导入——但要把一个**拒绝档**抬成高分，
> 仍然必须删 `sample_hit_rate`（本轮已封）或伪造 `sampled/matched`（R2），没有第三条路。这与 A-12 的 R4 边界一致。

---

## 7 · 合并建议

### **可以合并。**

理由（按 BA 的判定尺度逐条）：

1. **R5 真封**：A-13 的 4 条判据全部符合；原样攻击链在 import 第一道闸 exit 1，apply 第二道闸也独立复现；
2. **没有同类新洞**：三种删/改证据变体里，B/C 被封，A 是 A-16 已登记的「伪造全套自洽证据」，按 BA 明定的尺度**只登记、不阻断**；
3. **不退化**：9/9、4 份产物幂等、客户视图 0 泄漏 + 逐字节相同、32 套件 313、六红线 + `guard()` + `property_filter.rs` diff 空、零新依赖、零删测试；
4. **文档到位**：§11 存在且内容对得上，32 套件名单可逐套件复核，`source_enumeration` 真进了回归集并实跑通过。

### 合并时必须带上的记录要求

1. **R2/R3/R4 写进交付物「已知限制」**——§11.4 已经写了，保持；⛔ 不要写成「已封堵」。
2. **R5 的结论只写到「删采样证据已封」**，不要扩展成「置信度不可伪造」；R2 的反例本轮已实测（P0-2 A）。
3. 如果后续要收 R2/R4，建议单开一条 REQ（「按权重重算 + 要求五信号齐全 + 可选 digest 回源复核」），不要塞回本 REQ 再拖一轮。

---

## 8 · 复现命令与证据路径

```powershell
cd D:\MyCodes\AI\an_onto_demo\.review-workspace\req108\qa3
$env:CARGO_TARGET_DIR='D:\MyCodes\AI\an_onto_demo\.review-workspace\req108\qa3-target'
$env:ONTOK_DATABASE_URL='postgresql://…/ontok_demo'
$env:ONTOK_TEST_DATABASE_URL=$env:ONTOK_DATABASE_URL
$env:ONTOK_SOURCE_DATABASE_URL='postgresql://…/anspire_open_uat'   # 只读，口令只走环境变量
$env:SRC_URL=$env:ONTOK_SOURCE_DATABASE_URL

# ① 勘查 + 两次运行（确定性）
pwsh -NoProfile -File ..\qa3-out\run_propose3.ps1
# ② A-13 四条判据（真 CLI）
pwsh -NoProfile -File ..\qa3-out\run_crit.ps1
# ③ P0-2 三种变体（含 R2 全链路）
pwsh -NoProfile -File ..\qa3-out\run_attack2.ps1
# ④ 客户视图（两个不同分数项目）
pwsh -NoProfile -File ..\qa3-out\run_views.ps1
# ⑤ 32 套件 + lib/doc（跑前临时挪开 preflight，跑完挪回）
pwsh -NoProfile -File ..\qa3-out\run_suites3.ps1
# ⑥ 9/9 逐条（真源库）
cargo test --test test_req108_live test_req108_ac006_nine_known_errors_all_rejected -- --nocapture
```

**证据留痕**：

| 内容 | 路径 |
| --- | --- |
| 两次运行产物（4 份 ×2） | `.review-workspace/req108/qa3-out/run1/`、`run2/` |
| A-13 四条判据的输入与逐条日志 | `.review-workspace/req108/qa3-out/crit/`、`criteria.log` |
| P0-2 三种变体 | `.review-workspace/req108/qa3-out/attack2/` |
| 客户视图 / 建模视图 | `.review-workspace/req108/qa3-out/views/` |
| 32 套件逐套件日志 | `.review-workspace/req108/qa3-out/suites.log`、`suites-stdout.log` |
| 9/9 逐条实测 | `.review-workspace/req108/qa3-out/live-ac006.log` |
| apply 第二道闸 | `.review-workspace/req108/qa3-out/apply-attack-confidence.json`、`apply-proposal.json` |
| 我审的 worktree | `.review-workspace/req108/qa3`（detached `d4e3c68`，tree 同 feature tip） |
| 本轮报告 | `BA/demands/REQ-108/qa-review-3.md` |

---

## 9 · 无法确认项（如实说）

1. **全量 `cargo test` 的绿**：本机仍不可证（`src/bin/ontok-gate-preflight.rs` 的 `std::os::unix` 在本机编不过，跑任何 `cargo test` 前都必须把它挪开）。
   本轮只跑名单内目标：**32 套件 313 + lib 230 + doc 4 = 547 passed / 0 failed**；全量仍需 Linux/CI 补跑一次。
2. **9/9 的语义对错**：评分卡只回答「有多可信」；E1–E9 为什么错，是人判的（AC-006 的原始结论），本轮复核的是「仍落拒绝 / 仍不产出」。
3. **R2 的定性是政策判断**：它是不是「可接受边界」由 BA 在 A-16 定；我按 A-16 判非阻断，并明确它**没有被本轮修复**。
4. **源库快照依赖**：所有数字只对 `2026-10-21T09:38:12Z` 的 `anspire_open_uat` 成立（96 基表 / 1116 列 / 0 外键 / 95 单列主键）；与上一轮 schema 去掉时间戳后完全相同，所以两轮可比。
5. **「回归集」的强制力**：本仓无 CI 配置，32 套件是一份文档名单；本轮已逐套件执行并通过，但没有机器在每次提交时强制跑它。
