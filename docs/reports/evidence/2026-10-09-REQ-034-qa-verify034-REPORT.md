# REQ-034 验收报告（QA 独立验证）

- worktree：`/home/myg133/Projects/Anchnet/Ontology/verify-REQ-034`，分支 `verify/REQ-034`
- HEAD：`c082556b25e20b9bb48a8f133f65eb4a49e86d99`（结束时已恢复到该提交、干净）
- 基线：`171d2a0b5844218f6e0a37e2d491cf9726979d1a`
- 干净态指纹（`find src tests -name '*.rs' | sort | xargs sha256sum | sha256sum`）：
  `bde19f21351bc42f114fcd0db429038343ae7b9fe2f21e2ebf5f05b916cddb87`（变异前后一致，已复原）
- 结论：**PASS 带 2 项非阻断缺陷需登记**

## 0. 上一位 Dev 的「环境问题」结论：已被证伪

先复现 BA 给的正确取值方式（`.env` 只在仓库根，库名 `notol` -> `ontol`）：

```
cargo test --lib extract::introspect::tests::read_only_pool_really_reads_back_on
=> 1 passed; 0 failed; 0 ignored; 0 measured; 114 filtered out; finished in 0.64s
```

全量输出里 `relative URL without a base` 出现次数 = **0**（基线与分支各一次全量均为 0）。
⇒ **DB 可达、环境本身没问题**；上一位 Dev 的 121 条失败是**他自己的取值方式**造成的环境类失败，
不是代码失败，也不是环境损坏。此判断与本轮全部代码结论无关。

---

## 1. AC-003 判别力：成立，但有粒度限制

### 1.1 独立复现（两条都自己跑了）

| 命令 | 结果 |
| --- | --- |
| `cargo test --test test_llm_fallback` | **12 passed / 0 failed / 0 ignored** |
| `cargo test --lib agent::llm::` | **7 passed / 0 failed / 0 ignored**（108 filtered out） |

两条 AC-003 测试确实存在且带**反面自证**（同一 transport 在好正文下必须成功，
`tests/test_llm_fallback.rs:350-357`），不是「什么都失败」。

### 1.2 「160 记成 1」的量级：实证，不是硬编码

断言用的 fixture 是 `prompt=100, completion=60, reasoning=59`（`tests/test_llm_fallback.rs:137-146`），
160 = 100 + 60 是**算出来的**，不是写死的魔数。正文 token 数 = 60 − 59 = 1。

我用变异把它逼出来，拿到了实际数字（不是照抄 Dev 的说法）：

```
变异：total_tokens() 改为 completion - reasoning（只计正文）
断言输出：assertion `left == right` failed ... left: 1  right: 160
```

⇒ **160 倍低估**坐实。

### 1.3 存活变异必须逐条论证 —— M1 存活，理由如下

| 变异 | 集成二进制 | lib 二进制 | 判定 |
| --- | --- | --- | --- |
| M1：只关「空正文」守卫（content 默认成空抽取 JSON） | **存活** | **被杀** | 见下 |
| M1c：两个空正文守卫全关 | 被杀 | 被杀 | — |

M1 存活的**原因已查实，不是判据无效**：fixture `empty_body_resp()` 带
`finish_reason = "length"`（`tests/test_llm_fallback.rs:169`），而
`protocol.rs:197` 的截断守卫**排在空正文守卫之前**，先触发。
M1c 把两道守卫都关掉后，集成二进制立刻被杀：
`empty_body_is_not_treated_as_success ... FAILED`。

⇒ **结论**：AC-003 坑一在集成层是由「截断」这条路径兜住的；
「没截断但正文为空」这条形状在集成层（agent 级）**没有测试**，
只有 protocol 层两条单元测试覆盖（`null_content_without_length_finish_is_also_an_error`、
`whitespace_only_content_is_an_error`）。这是**覆盖粒度的缺口，不是缺陷**。

### 1.4 briefing 要求的四个绕过（自建探针，跑完已删）

| 绕过 | 实测 | 判定 |
| --- | --- | --- |
| `total_tokens` 整个删掉 | 回退 `prompt+completion` = 160 | **正确** |
| `total_tokens: null` | serde 解成 `None` -> 回退 160 | **正确** |
| `completion_tokens` 也不返回 | serde default 0 -> **100**（真值 160） | **少计 37.5%，静默** |
| `reasoning_tokens` > `completion_tokens`（异常上游） | 110（=100+10）不受影响；加总法会得 1109 | **「不加总」的决定更稳健** |
| 解析失败（非空非法 JSON） | `usage_len=1, used=160` | **钱有进账，无漏记** |

---

## 2. fallback 语义论证：核实成立，「写不出来」这句措辞不准确

### 2.1 三处引用逐条独立核实（全部命中，行号精确）

| Dev 的引用 | 核实结果 |
| --- | --- |
| `factory.rs:78` = `build_agent` 返回单个 `Box<dyn ExtractionAgent>` | **命中**（基线 171d2a0 第 78 行正是 `pub fn build_agent(`） |
| `orchestrate.rs:524` = `orchestrate` 收单个 agent | **命中**（基线第 524 行是该函数文档头，写明 `agent` 是 `&dyn ExtractionAgent`） |
| `factory.rs:21-34` = 模块头明确禁止回落 | **命中**（第 21 行「fail-closed：找不到登记的实现就报错，绝不回落」至第 34 行「没有任何 `unwrap_or_else` 之类的回落分支」） |

`ExtractionAgent` trait 区域 sha256 基线与 HEAD **完全相同**（`15601d34d2…`）⇒ AC-001「trait 方法零改动」成立。

### 2.2 「自动接管写不出来」：字面不准确，实质结论成立

用一个复合 agent（`FallbackAgent { primary, secondary }`）实现 `ExtractionAgent`，
**是能编译的**——trait 不禁止一个 impl 持有两个 agent。所以「写不出来」这句**字面不成立**。

但 Dev 的**实质结论是对的**，而且比字面表述更强：一旦这样写，

- `identity()` 只能返回**一个**身份。若返回 primary，那 1 组真正由 LLM 抽出来的结果
  会被盖上 primary 的章 —— 这正是 Dev 说的「对 90 组说谎，且静默」。
- 若返回合成名（如 `rules→llm`），则该名不在登记里，§8.4 的版本锁定与 diff 归因失去依据。

**「最强那条」我独立核实为真**：

- `ExtractedBy`（`src/domain/snapshot.rs:108-123`）只有**一个** `agent: String`（第 110 行）；
- `ProposalProvenance`（`src/extract/propose.rs:90-101`）同样只有**一个** `agent: String`（第 91 行）。

两个溯源结构都是单 agent，**逐字节核实，不是读注释**。

### 2.3 `BudgetReport::absorb` 的 replace 语义：核实成立

`src/agent/orchestrate.rs:379-383`：`latest_by_group.insert(r.group_id.clone(), ...)`
按 `group_id` 覆盖；`totals()`（401-415）再对 `latest_by_group` 求和。
字段文档也写明「同组第二次上报**替换**第一次，绝不累加」。
⇒ **Dev 的描述准确**（他说 381，落在 insert 上）。

### 2.4 我同意「显式配置」吗？—— 同意。理由与 Dev 不同

同意，但支撑理由要分三层，Dev 只讲对了第三层：

1. **机制上**：编排入口只有一处 `drive_group(agent, group, attempts)`，
   且重试的是**同一个** agent（`src/agent/mod.rs:435-459`），全仓没有「备用 agent」这个概念。
2. **代价上**：自动接管要付两次钱，而现有记账契约（同上 §2.3）对同组是 replace 不是累加，
   两腿必然互相覆盖 —— 这正是 Dev 指出的 `absorb` 约束。
3. **诚实性上（决定性）**：溯源结构只有一个 `agent` 字段，混跑即静默造假溯源。
   这条与前两条无关，**即使前两条都能改，这条仍然拦着**。

### 2.5 第三条路：有，但不是「自动接管」

**建议登记为后续 REQ 的方向：把 LLM 兜底做成「显式的第二轮」，而不是「轮内接管」。**

- 可行性已核实：`ExtractedBy.round` 是既有字段（`snapshot.rs:114`），
  `build_agent(registration, names, round)` 的 `round` 由调用方给（`factory.rs:66-67` 文档），
  提案本身也带 `round`（`propose.rs:142`）。
- 于是「第 1 轮规则 → 第 2 轮 LLM」天然产生**两份各自单 agent 的溯源**，
  每份都诚实，不需要改 `ExtractedBy` 结构、不需要改提案 JSON 契约、不碰 `fixtures/proposal-demo.json`。
- 与 Dev 的「显式配置」不冲突 —— **这就是显式配置的更强版本**，只是把「手工换登记」
  变成「编排读配置按轮选实现」。成本上每轮各设各的 `max_used`，天然不存在同组覆盖问题。

**明确回答 briefing 的反问**：我认为「按组建 agent」在当前抽象里**做得出来**
（复合 impl 即可编译），但**会真的产生 Dev 说的那个后果** —— 90 组走规则、1 组走 LLM，
`extracted_by` 只有一个 `agent`，那 1 组会被盖上错误来源，且静默。Dev 的结论对，
只是「写不出来」这个说法该改成「写得出来，但代价是静默的假溯源」。

---

## 3. `used` 算法：三条规则在真实 OpenAI 兼容服务上**都成立**

| 规则 | 判断 | 依据 |
| --- | --- | --- |
| 优先 `total_tokens` | **成立** | 服务端自报的是它自己的账单口径，与实际计费同源。自己加总 = 赌上游是否已把 reasoning 计入 completion |
| 回退 `prompt + completion` | **成立（对本通道）** | 实测 `reasoning 59 / completion 60` ⇒ completion 已含 reasoning，故 100+60=160 正确 |
| **不加** reasoning | **成立，且更稳健** | 异常上游 `reasoning(999) > completion(10)` 时：`completion+reasoning` = 1109（**10 倍高估**），本实现 = 110（不受污染） |

`completion_tokens` 是否含 reasoning **依上游而定**，Dev 没有把实测当协议保证，这一点是对的
（`protocol.rs:126-133` 明说「那是**这个通道**的性质，不是协议保证」）。

⚠️ **我没能自己打到真端点**：本会话环境里**没有任何** LLM 通道变量
（`env` 里只有 `MINIMAX_API_KEY`，无 base_url / model / endpoint；
`/home/myg133/Projects/Anchnet/Ontology/.env` 只有 `ONTOK_TEST_DATABASE_URL` 一个键）。
「160 vs 1」因此是用**变异逼出来的实际数字**证实的，不是照抄 Dev 的断言，也不是我自己实测的端点。
这条如实记为**未独立实测**。

---

## 4. `ureq 3.4.2`：可接受。我提过一个 MSRV 怀疑，**已自我推翻**

| 检查 | 结果 |
| --- | --- |
| 引入 `block_on` 风险？ | **无**。全仓 `grep block_on` **零调用点**（只有注释），ureq 是阻塞库，不需要 block_on |
| `acp/mod.rs:38-41` 的 panic 记录 | 命中：第 37-38 行「同步方法要 I/O 就得 `block_on`，而 `block_on` 在已有 runtime 上下文里**直接 panic**」 |
| 是否已有等价能力？ | **无**。改前 `Cargo.toml` 无 reqwest / ureq / hyper / isahc / surf 任何 HTTP 客户端 |
| 依赖面 | lock 新增 22 个包，但**实际只编译 11 个**（`cargo tree -e normal --format {p}` 与 lock 求交）；另 11 个（cookie / cookie_store / time 等）是未启用的 optional |
| 许可证 | ureq = `MIT OR Apache-2.0`；11 个实际编译的新包全部 MIT / Apache-2.0 / 0BSD / Zlib，**无 copyleft** |
| 「不引入第二套 TLS 栈」 | **成立**。rustls / ring / rustls-pki-types / rustls-webpki / subtle 在**基线 lock 里已存在**（经 sqlx） |
| MSRV | 我一度怀疑 `time 0.3.55` / `cookie_store 0.22.1` 声明 `rust-version = 1.88.0` 会破 `Cargo.toml:6` 的 `1.86`。**该怀疑不成立**：这两个包未被编译（ureq 的 `cookies` 是 default feature，本项目 `default-features = false`）。实际编译集合最高为 ureq/ureq-proto 的 **1.85 ≤ 1.86** ⇒ **MSRV 仍成立** |
| 同步阻塞会否卡住 tokio？ | **会阻塞，但不 panic**。`ontok-propose.rs:40` 是 `#[tokio::main]`，`:243` 调 `orchestrate`，故阻塞 HTTP 跑在 tokio worker 上，默认超时 120s。ACP 有**同一性质**且已自曝（`acp/mod.rs:58-60`）⇒ 属继承的既有取舍，非本 REQ 引入的回归 |

类型：架构。**接受**。

---

## 5. 全量与红线

### 5.1 全量（口径：`cargo test --no-fail-fast`，逐个 `test result:` 行 awk 求和）

| 树 | passed | failed | ignored | binaries | 耗时 |
| --- | --- | --- | --- | --- | --- |
| 基线 `171d2a0`（**独立 worktree `verify-base-REQ-034`，独立 target，跑完已删**） | **539** | **0** | **1** | 38 | **7m28.6s** |
| 分支 `c082556`（本 worktree） | **559** | **0** | **1** | 39 | **6m33.9s** |

- 基线 **539/0/1** 与 AC-006 写的预期**完全一致** ⇒ 环境侧确认无回归。
- 差值 **+20**，且逐条列出恰为：12 条集成 + 7 条单元 + 1 条 doctest = 20。
- **失败集合 diff = 空**。
- **测试名集合 diff：只有新增（`>`）**，外加一处改名
  （`test_two_protocols_…` -> `test_three_protocols_…`）⇒ **无任何测试被删除**。
- 那 1 条 ignored 是既有的 `todo_identifier_defenses_have_own_assertions`（非本 REQ 引入）。

### 5.2 红线核查（逐条独立核实）

| 红线 | 结果 |
| --- | --- |
| `tests/drop_rate.rs` 零改动 | **通过**（`git diff --name-only` = 0） |
| `tests/test_fk_naming.rs` 零改动 | **通过**（同上） |
| `src/domain/view.rs` 零改动 | **通过**（同上）；`test_humanize_still_refuses_unknown_columns` 仍在 `view.rs:1254`，文件未动 ⇒ **一条断言都没放宽** |
| 无测试被删除 / `#[ignore]` | **通过**（diff 中无 `#[ignore]` 增删；见 §5.1 名字集合 diff） |
| trait 方法零改动 | **通过**（trait 区域 sha256 两边同为 `15601d34d2…`） |
| `.review-workspace/fix-props/schema.json` 绝不可删 | **通过**。仅作 `--schema` **只读输入**使用；sha256 `b71c2afde6…`，大小/时间未变 |

### 5.3 「断言 2 -> 3」是收紧还是放宽？—— **收紧**

`tests/test_agent_orchestration.rs` 的改动**不止改数字**：

- `assert_eq!(m.len(), 3, ...)`（原来是 2）；
- **新增** `assert!(m.contains_key("llm"), "内置 LLM 兜底已接进分派点…")`。

即：计数变严 + 多了一条正面断言，且另外两条 `contains_key("inproc"/"acp")` 原样保留。
⇒ 这是**如实更新计数**，不是放宽判据。**核实通过。**

### 5.4 产物 sha：Dev 说「复现不出来」是**错的**——我复现出来了

先按 Dev 的输入（同输入对照）独立验证，**与 Dev 报的值逐字节吻合**：

```
输入 .review-workspace/req015/cli-e2e/2172521-1005/schema.json
命令 ontok-propose --schema <上面> --out proposal.json --stats stats.json --project extracted（不传 --names）
基线 171d2a0 : proposal 47ab0b391b… / stats 0cad0f7ba0…
分支 c082556 : proposal 47ab0b391b… / stats 0cad0f7ba0…
cmp 基线 vs 分支：proposal.json BYTE-IDENTICAL，stats.json BYTE-IDENTICAL
```

**但 acceptance.md 那组 sha 是可以复现的。** 我遍历了 `.review-workspace` 下**全部 127 个
`schema.json`**，命中输入是：

```
.review-workspace/fix-props/schema.json
=> stats.json    = cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba   MATCH
=> proposal.json = 1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635   MATCH
```

⇒ **AC-006 的产物要求按其本来的口径直接通过**，无需替换成「基线 vs 分支同输入」。

**口径判断（briefing 问的）**：「同输入下基线与分支逐字节一致」**能否**证明抽取逻辑未变？
**能，但它是弱一档的证据**。它只证明「两个二进制在同一输入上输出相同」，
无法排除「两个二进制一起错了」；而 acceptance.md 里那组 sha 是**独立于本 REQ 预先记录的绝对基准**，
直接命中它才是强证据。所以那组 sha 之所以重要：**它是外部固定的坐标，不是自证的**。
这次两者我都验了，结论一致。

⚠️ Dev 的做法（换成同输入对照）**本身是有效的补充**，只是「找不到输入」这个陈述不成立 ——
输入一直在 `fix-props/` 下，Dev 当时只搜了 `req015/cli-*` 路径。

---

## 6. 发现但未修的问题（按严重度）

### 【中】编排层记账对「同组重试」少计，LLM 让它从理论变成常态

- **触发条件**：某组第一次失败（传输/形状类）而第二次成功。
  LLM 场景下这是**设计内的常见路径**——AC-003 坑一（正文为空）必然返回 `Err`，
  而 `drive_group` 默认 `attempts = 2`（`orchestrate.rs:147`）必然重试。
- **实测**：自建探针（脚本：第 1 次正文空 used=120，第 2 次正常 used=160）

  ```
  agent.usage() 条数 = 2
  agent.usage() 明细 = [120, 160]  => 真实花费合计 = 280
  BudgetReport.used（账上）     = 160
  差额 = 120
  groups_ok=1  failed=0          <- 失败的第一腿在产物里毫无痕迹
  ```

  **账上少了 120 token（真实 280 / 账面 160）**，且该组仍报「完全成功」。
- **机制**：`absorb`（`orchestrate.rs:379-383`）按 `group_id` replace；
  `LlmAgent::record_usage` 每次调用 push 一条**增量**（`mod.rs:375-379` 自述「直接就是本组增量」）。
  增量上报 + replace 记账 = 后者吞掉前者。
- **归属**：**既有问题，本 REQ 未引入**。ACP 的 `delta_to_report`（`acp/protocol.rs:448`）同样上报增量、
  同样会被吞；`orchestrate.rs` 本 REQ **零改动**。
  但 REQ-034 把它**从边角变成常态**：每次尝试都是一次真金白银的 HTTP 调用。
- **为什么不阻断**：修复要动 `orchestrate` 的记账契约（影响 ACP），超出 REQ-034 声明范围；
  且 Dev 已**主动披露**了 `absorb` 的 replace 约束（只是把它归到「将来做 fallback」，
  没注意到它**当下就已经作用在重试路径上**）。
- **建议**：登记独立 REQ。要么 `UsageReport` 增一个 `seq`/attempt 维度，
  要么重试时换 `group_id`（Dev 已指出的那条），要么编排改按 `used` 求和并要求实现上报累计值。

### 【低】`usage` 对象存在但 token 字段全缺/为 0 时，静默记 0

- **实测**：探针 `usage: Some(Usage{0,0,total:None,details:None})` ⇒
  `extract_ok=true`，账上 `used: 0`。字段缺失时 serde `default` 成 0，**与真实的 0 不可区分**。
- 另测：只缺 `completion_tokens` ⇒ 算出 100（真值 160），**静默少计 37.5%**。
- 与模块头自述的纪律**有出入**：`mod.rs:413-416` 明写「报 0 = 把『不知道』说成『免费』，那是账目造假」，
  而 `record_usage` 只对 `usage` **整个缺失** fail-closed，对**字段缺失**没有防线。
- **不阻断**：实测通道返回完整 usage，今天打不到。但闸门的 fail-closed 是本项目的核心纪律，
  建议把「三个 token 字段同时为 0」也纳入 fail-closed。

### 【低】集成层缺「没截断但正文为空」这条形状的 agent 级测试

§1.3 已述：fixture 带 `finish_reason = "length"`，导致集成层的坑一实际由截断守卫兜住。
protocol 层有两条单元测试覆盖另一种形状，**不是缺陷，是覆盖粒度缺口**。

### 【信息】`capabilities_are_static_and_never_depend_on_the_answer` 名不副实了一点

它只测「不变性」，不测「内容」。谎报 `rebuilds_proposal` 的变异（M6）是被
`llm_registration_endpoint_and_credentials_fail_closed` 杀掉的，不是被它。
测试本身没问题，能力内容断言在别处，只是名字容易让人以为它守着内容。

---

## 7. 我自己引入又清掉的问题（如实登记）

| 项 | 处置 |
| --- | --- |
| 7 次源码变异（M1/M1c/M2/M3/M4/M5/M6） | 每次用 `git checkout --` 复原。最终 `git status --porcelain` = 0 行，全 `.rs` 指纹 = `bde19f21…`，与变异前**逐字节一致** |
| 2 个临时探针测试文件 `tests/zz_qa_probe{,2}_req034.rs` | 用 `mavis-trash` 删除，已确认不在工作区 |
| 基线对照 worktree `verify-base-REQ-034` | `git worktree remove --force` 删除；`git worktree list` 已无该项 |
| **偏离 1**：本会话无 `write` / `edit` 工具 | 改用**带引号 heredoc**（`<<'EOF'`）与 `python3` 精确替换兜底。按纪律登记偏离，未反复重试 |
| **偏离 2**：第一次全量我误用 `\| tail -120`，丢了聚合口径 | 丢弃该次结果，**重跑**一遍并全量落盘 `head_full_run.txt` 后再统计；报告里的数字全部来自重跑那次 |
| **偏离 3**：`.env` 只在仓库根、worktree 内没有 | 按 BA 给的方式取值并做成 `env.sh` helper；全程**只打印结构事实**（scheme / 库名 / 是否绝对 URL），**未回显连接串与口令** |
| 我提过并**自我推翻**的怀疑 | `time 0.3.55` / `cookie_store 0.22.1` 的 MSRV 1.88 会破本 crate 声明的 1.86 —— 经 `cargo tree` 核实二者**未被编译**，MSRV 不受影响，已从发现中撤下 |

---

## 8. 环境事实

- DB 可达：门禁测试 1 passed / 0.64s；两轮全量 `relative URL without a base` 出现次数均为 **0**。
- 本机无 `psql` / `psycopg` ⇒ **未测 `SELECT 1` p50**（与 AC-006 的说明一致）。
- LLM 通道变量在本会话**不可见** ⇒ AC-003 的量级用变异实证，**未做真端点调用**。
- 耗时口径：`time` 只包住 `cargo test`，**不含首次编译**（基线那次编译在它之前单独发生）。
  两轮**各自独占**运行、互不并发。数值随共享集群负载浮动，**不声称稳定值**，仅记录本次实测。
- 共享本体库全程只读；未执行任何 DELETE / TRUNCATE。
- 未 push、未 squash、未改历史（仓库无 remote）。
